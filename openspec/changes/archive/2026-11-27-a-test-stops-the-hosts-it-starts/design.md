## Context

A test of sluice starts `sluice host` through a command of sluice. The
command starts the host in a session of its own. The guard of the test
support finds the host by the test's state folder, and stops it when the
test's folders drop (`a-leaked-child-fails-its-test@9a82a5147ba6`). A
test process that a signal ends runs no drop.

## Decisions

### D1. A test process that a signal stops stops its host processes

Each `Scratch` puts its folder on a list of the process when the test
support makes it, and takes it off when it drops. The first one also starts a thread and
sets a handler for `SIGINT`, `SIGTERM` and `SIGHUP`. The handler writes
the signal's number to a pipe, and does nothing more. The thread reads
the pipe. It finds the host processes of the listed folders, as the
guard finds them, and sends `SIGTERM` to each. Then it sets the default
action of the signal and raises the signal again. So the process ends
by that signal, and nextest reports the test as it did before.

The thread does not wait for the hosts to end. While it waits, the
test's body continues, and the test can pass. A host that gets
`SIGTERM` stops by itself and ends its workers. The other processes of
the test are in its process group, and nextest signals that group.

### D2. The leak script fails a task that leaves a host process

When the command ends, `tests/leaks.sh` gives each host process of the
test one second to end. An idle host stays for a moment after its last
client (`an-idle-host-process-ends`). The test left each host that still
runs. The script names it, sends `SIGTERM`, waits for it, and
fails.

A host's version file gives its pid and its program, and the file stays
after the host ends. Another process can then hold that pid. So the
script reads a pid as the host only while the pid's command line is the
program with `host` or `daemon`. It reads the command line with
`pgrep -alf`, as it names each other process.

The script runs the command in a process group of its own. A signal to
the task's group does not get to the command. So the script sends each
`SIGINT`, `SIGTERM` and `SIGHUP` that it gets to the command's group. A
trapped signal ends the shell's `wait`, so the script waits again while
the command runs.

### D3. The crate's test tasks run under the leak script

`test`, `tools`, `sandbox` and `speed` of `crates/sluice/moon.yml` run
cargo-nextest under `tests/leaks.sh`. So a test process that `SIGKILL`
ended, which runs no code, still fails the task, and its host stops.
Each task has `tests/leaks.sh` as an input. The other crates start no
`sluice host`, and their tasks stay as they are.

## Risks

- The script's folder adds 10 characters to each path under `TMPDIR`. A
  socket path holds 103 characters at most. The `e2e` task runs under
  the script already, with the same `TMPDIR`.
