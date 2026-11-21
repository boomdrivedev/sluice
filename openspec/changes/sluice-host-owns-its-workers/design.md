# Design

## Context

The tests of the `sluice` crate start the binary with a state folder and scratch folders of their own. A test's scratch folders are removed when the test drops them. A host process ends at its next tick after its state folder is gone. So most host processes that a test starts end soon after the test. Nothing makes the test fail when one does not end.

Some processes escape a test today:

- A command starts the host process in a session of its own.
- A host process starts each run's supervisor in a session of its own.
- A hand-over starts the successor in a session of its own.

A detached process takes no part in its test's process group, and its output goes to the host process's log. So a runner that watches the test's output does not see it.

This change ends those detachments and adds the guard that proves it. See the proposal for why.

## Goals / Non-Goals

**Goals:**

- Each test that leaves a process fails, in each Rust crate and in each shell test task.
- No process of a test runs after the test's guard is done.

**Non-Goals:**

- A rule for the plugins' own code. Only their tests change.

## Decisions

### D1. A guard that finds a test's processes by its folders

The guard holds the test's state folder and scratch folders. When the test ends, it lists the user's processes with their environment and their working folder. A process belongs to the test where its `XDG_STATE_HOME` is the test's state folder. It also belongs to the test where its working folder is in one of the test's scratch folders. The host process keeps the environment of the command that started it, so the guard finds it. A run's checks work in a place in the test's repository, so the guard finds them too. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

The guard lists processes with the sysinfo crate. sysinfo reads a process's environment and working folder on macOS and on Linux for the user's own processes.

Not taken: a variable that the guard sets and that each process inherits. A run's checks get a run's environment, so the variable would not reach them unless sluice kept it for tests. Not taken: the process group or the parent. A detached process has neither of the test's.

### D2. Stop the host processes first, then judge

At the test's end, the guard first sends `SIGTERM` to each `sluice host` of the test and waits for it to exit, up to ten seconds. A test that ends with its host process running is normal. A test that ends with a run, a supervisor or a git command still running is a leak. The guard then lists the test's processes again. It fails the test where any is left, and ends each of them, `SIGTERM` first and `SIGKILL` after two seconds.

The guard reports the failure after it ends the processes, so a failed test leaves nothing behind.

Not taken: a test that must stop its own host process. Each test would then repeat the same stop, and a test that panics would skip it.

### D3. The guard is part of the test's setup

`OnDisk` in the test support holds a guard. Each test that builds an `OnDisk` gets the guard with no other line. A test that starts sluice without `OnDisk` builds a guard of its own from its folders. The guard's check runs in `Drop`. Where the test already panicked, the guard ends the processes and does not panic again.

### D4. cargo-nextest for the Rust crates

The `test`, `tools` and `sandbox` tasks of the five crates run `cargo nextest run` with the same filters. A nextest profile at the root sets `leak-timeout` to `{ period = "500ms", result = "fail" }`. nextest runs each test in a process of its own, so a test that leaves a process that holds its output fails. The devenv shell pins cargo-nextest, as it pins the tree's other tools.

Not taken: the guard alone. It does not see a process that a test starts outside its folders, such as a plain `sleep`.

### D5. One shell wrapper for the shell test tasks

A script, `tests/leaks.sh` at the root, runs a test command with a new scratch folder and state folder in its environment. When the command ends, the script lists the processes of those folders by their environment and their working folder. It fails where one is left, and ends it. The plugins' shell test tasks and the check-e2e tier run through it.

## Risks / Trade-offs

- [A test that leaves a run now fails.] → The change's tasks run each suite one at a time. Each failing test then waits for its runs to end, or withdraws its submission, before it ends.
- [The guard's list of processes costs time at each test's end.] → It lists the processes once, or twice where a host process was stopped. sysinfo reads only the processes' environment and working folder.
- [nextest runs each test in a process of its own, so a test that shared state through statics in one process may act differently.] → The suites already run each test file as a binary of its own. The change runs each suite under nextest and fixes what fails.

## Migration Plan

The change replaces `cargo test` with `cargo nextest run` in the crates' tasks. A person who runs `cargo test` by hand still can. Only the tasks and `sluice exec` use nextest.
