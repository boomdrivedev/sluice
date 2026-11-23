## MODIFIED Requirements

### Requirement: Each worker is a child of sluice host in a process group of its own

Each process that sluice starts to do its work SHALL be a child of `sluice host` in a process group of its own: checks, a preview's command, a release, git, equip or credential command. `sluice host` SHALL record its group in the host's state before it does its work. (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

Before: a run that `sluice exec` started had no owner. Now: `sluice host` starts it.

#### Scenario: A combination's checks

- **WHEN** `sluice host` runs a combination's checks
- **THEN** the checks' parent chain reaches `sluice host`, their process group is not the group of `sluice host`, and the host's state names that group

#### Scenario: A session's checks

- **WHEN** `sluice exec main -- sleep 30` runs in a worktree
- **THEN** the parent chain of `sleep` reaches `sluice host` and not `sluice exec`, and the host's state names its group

#### Scenario: A preview's command

- **WHEN** `sluice preview main` starts a preview whose command runs `sleep 30`
- **THEN** the parent chain of `sleep` reaches `sluice host`, and the host's state names its group

## ADDED Requirements

### Requirement: A session's checks run in sluice host

`sluice exec` SHALL send its folder, its branch and its command to `sluice host`. `sluice host` SHALL equip the working copy, run the checks as its worker, and send their output as the checks write it. The client SHALL print it as it comes, and SHALL exit with the code that `sluice host` answers. (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`, `workers-end-with-their-owner@292e869158d6`)

Before: `sluice exec` equipped and ran its checks itself. Now: `sluice host` does both.

#### Scenario: The output reaches the client as it comes

- **WHEN** `sluice exec main -- sh -c 'echo one; sleep 5; echo two; exit 4'` runs in a worktree with only `sluice` on its `PATH`
- **THEN** the client prints `one` before the check prints `two`, then prints `two`, and exits with the code that a check that exits 4 gave before this change

### Requirement: A session's checks take a slot of the bound in sluice host

`sluice host` SHALL take a slot of `[runs] max` for a session's checks from its own room, in the order of the run bound, before it starts them. While they wait, it SHALL send the client the line that says why. (`runs-max-counts-checks-only@2409f7ec2f4f`, `workers-within-their-tools-limit@205585d82c6b`)

Before: `sluice exec` asked `sluice host` for room by `hold`. Now: `sluice host` takes the slot itself.

#### Scenario: A session's checks wait for a slot

- **WHEN** `[runs] max` is 1, a combination's checks run, and `sluice exec main` runs in a worktree
- **THEN** the client prints `waiting: the host is at its bound`, and the session's checks start only after the combination's checks end

### Requirement: A run's kept variables come from its caller

`sluice exec`, `sluice preview`, `sluice equip` and `sluice release` SHALL send the variables of their caller's environment that a run keeps. `sluice host` SHALL give the run those variables in place of its own.

Before: a session's run took the kept variables from the environment of the `sluice exec` that started it. Now: it takes the same variables from the request.

#### Scenario: The caller's PATH

- **WHEN** `sluice host` was started by a command whose `PATH` holds `/a`, and `sluice exec main -- sh -c 'echo $PATH'` runs with a `PATH` that holds `/b` first
- **THEN** the client prints a `PATH` that holds `/b` first

### Requirement: A session's checks end with their client

When the client of a session's checks ends, or its connection closes, `sluice host` SHALL end the run's process group. It SHALL give the run's slot back and SHALL record the run as stopped. A session's run that waits for a slot SHALL stop waiting when its client's connection closes. (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

#### Scenario: Ctrl-C on a session's checks

- **WHEN** `sluice exec main -- sleep 60` runs in a worktree, and the person presses Ctrl-C
- **THEN** within the grace of a stopped call no `sleep` of that command runs, and the run's slot is free
- **AND** the worktree's last result says that the run stopped

#### Scenario: A client killed while its checks run

- **WHEN** `sluice exec main -- sleep 60` runs in a worktree, and the client receives `SIGKILL`
- **THEN** within the grace of a stopped call no `sleep` of that command runs, and the run's slot is free

#### Scenario: A client stopped while it waits for a slot

- **WHEN** `[runs] max` is 1 and held, an agent's `sluice exec main` waits for a slot and is stopped with Ctrl-C, and the slot is then freed
- **THEN** the next agent's check that waits starts in it, and the stopped one is counted nowhere

### Requirement: A session's checks end with sluice host

When `sluice host` ends for a reason other than a hand-over, the session's checks SHALL end with it. The client SHALL say that sluice's host process ended, and SHALL exit 3. (`sluices-runs-end-with-its-host-process@84c331c3e64a`)

#### Scenario: sluice host stopped while a session's checks run

- **WHEN** `sluice exec main -- sleep 60` runs in a worktree, and `sluice host` receives `SIGTERM`
- **THEN** when `sluice host` has exited no `sleep` of that command runs, and the client exits 3
