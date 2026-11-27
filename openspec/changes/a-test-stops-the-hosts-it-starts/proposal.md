## Why

On 2026-11-27, nine `sluice host` processes of sluice's tests ran on one
host. A place's tests started them on 2026-11-26, and they ran for a day.
Each host held its socket and its state folder in the place's test
folders, and its memory. Each new run of the tests can add more.

The cause:

1. cargo-nextest sends `SIGINT` to each test that runs when a person
   or a tool interrupts the run, and `SIGTERM` to a test after its time limit. A Rust
   test process that gets one of these signals ends at once. It runs no
   drop. So the guard of the test does not stop the host processes of
   the test.
2. A command of a test starts `sluice host` in a session of its own, as
   it does on any host. The host does not end with the test process.
3. A test's host holds a submission that waits. A host process that has
   work ends only when a signal stops it (`an-idle-host-process-ends`). So
   the host runs on until a person stops it.
4. No check finds such a host. The crate's test tasks do not run under
   `tests/leaks.sh`. And `tests/leaks.sh` stops each host of a shell
   test, but it does not fail the task for that host.

We started one test of `peers.rs` and interrupted nextest after two to
four seconds, three times. Each time, the test's host ran on.

## What Changes

- The test support keeps a list of the live scratch folders of each test
  process. When the process gets `SIGINT`, `SIGTERM` or `SIGHUP`, it
  sends `SIGTERM` to each host process of those folders. Then the signal
  ends the process as before.
- `tests/leaks.sh` fails the task when a host process of the test runs
  one second after the test ends. It names the host, stops it and waits
  for it. A host that ends by itself in that second is no leak.
- `tests/leaks.sh` sends a `SIGINT`, `SIGTERM` or `SIGHUP` that it gets
  to the process group of the test. Then it continues when the test
  ends. Before, the test ran on in its own group.
- The test tasks of the `cli` project, `test`, `tools`, `sandbox` and
  `speed`, run under `tests/leaks.sh`, as `e2e` does.
- `tests/leaks.sh` reads a pid as a host only while its command line is
  the host's program with `host` or `daemon`. The version file of a host
  that ended stays, and another process can hold its pid.
- `tests/leaks-cases.sh` has four more cases. A test leaves a host
  process. A host process ends by itself. A version file names a pid
  that another process holds. A signal stops a test while its host
  process runs.

## Out of scope

- sluice itself does not change. A host process with work runs until a
  signal stops it. That is correct for a host process of a person.
- A test process that gets `SIGKILL` runs no code. `tests/leaks.sh`
  stops its host when the task ends. Nothing stops it when the task
  itself gets `SIGKILL`.

## Capabilities

### Modified Capabilities

- `test-children`: a test that a signal stops stops its host processes.
  The leak script fails a task that leaves a host process. The crate's
  test tasks run under the leak script.

## Impact

- `crates/sluice/tests/support/guard.rs`: `hold`, `let_go` and the stop
  on a signal.
- `crates/sluice/tests/support/mod.rs`: `Scratch` holds its folder while
  it lives.
- `tests/leaks.sh` and `tests/leaks-cases.sh`.
- `crates/sluice/moon.yml`: the four test tasks run under
  `tests/leaks.sh`, which is an input of each.
