## Why

Each pull request of sluice fails its checks on GitHub. Three tests
fail there on Linux. They pass in sluice's gate, which runs the same
tasks on a macOS host.

1. `exec::a_worktrees_release_runs_in_sluice_host` (`cli:tools`) fails
   each time. It says that the release command is not under
   `sluice host`. The release is a string, so `sluice host` starts it
   as `/bin/sh -c`. On Linux, `/bin/sh` is dash. dash starts the
   script as a child of its own. The script writes `$PPID`, the pid of
   dash. The test reads the parents of that pid after the release
   ends. dash ended with the release, so the test reads no parent. On
   macOS, `/bin/sh` replaces itself with the script, and `$PPID` is
   `sluice host`, which still runs. `sluice host` owns the release on
   both systems.
2. `plugin-herdr:test-dash` fails two watch tests each time. The
   devenv shell on GitHub sets `COLUMNS` to 80. procps `ps` cuts each
   line to `COLUMNS`. The test reads the command line of the source as
   `/bin/sh <checkout>/plugins/herdr/tests/fake`. The words that tell
   the source, `overview --json --watch --stdin`, are cut off.
3. `root:leaks-cases` fails each time. The guard, `tests/leaks.sh`,
   names a process with `pgrep -lf`. procps `pgrep` gives the name of
   the process alone, `sleep`. The case expects `sleep 61`. On macOS,
   `pgrep -lf` gives the whole command line.

The gate runs all three tasks. It runs them on the one host that its
policy names as a signer, and that host is a Mac. So no check of the
gate runs on Linux. GitHub's Linux job is the one Linux run of each
pull request.

## What Changes

- The release test gives the release as a list. `sluice host` starts
  the script, so the parent that the script writes is `sluice host` on
  each system.
- The herdr plugin's tests read the children of a process with
  `ps -ww`, which gives the whole command line whatever `COLUMNS` is.
  Their `pgrep` fallback adds `-a`.
- The guard names a process with `pgrep -alf`. procps `pgrep` then
  gives the whole command line. On macOS, `-a` lets the ancestors of
  `pgrep` match too, and the guard names only the pids it found.

## Capabilities

### Modified Capabilities

- `test-children`: a test reads the whole command line of a process,
  and reads the parents of a process that still runs.

## Out of scope

- **A Linux host in the gate.** For the gate to run on Linux, the
  policy needs a signer that runs on Linux, a pool or a host. That is a
  change to the policy and to a host, not to the tests.
- **A source that ends after the guard looks.** In a Linux container,
  each group of the herdr plugin's tests leaves the fake source and its
  `sleep 1`. They run for up to one second after the group ends. The guard looks
  at once and names them. They end alone within three seconds. GitHub
  has not shown this.

## Impact

- `crates/sluice/tests/exec.rs`: `a_worktrees_release_runs_in_sluice_host`.
- `plugins/herdr/tests/run.sh`: `children_of`.
- `tests/leaks.sh`: the names of the processes that it found.
