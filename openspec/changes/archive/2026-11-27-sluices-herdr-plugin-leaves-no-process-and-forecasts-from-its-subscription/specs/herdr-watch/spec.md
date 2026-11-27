## ADDED Requirements

### Requirement: The watcher ends with the herdr server that started it

The watcher SHALL be the command that a herdr server started, and a child of that server for all of its life. When that server ends, the watcher SHALL stop with its sources. Before it stops, it SHALL ask another server that it serves to start the next watcher by the plugin's action `watch`. A popup's command SHALL NOT become the watcher. (`nothing-detached@f807c7f5fcea`)

#### Scenario: The server that started it ends

- **WHEN** the watcher that server one started also serves server two, and server one ends
- **THEN** the watcher and its sources stop, and server two is asked to start the next watcher by `plugin action invoke watch`

#### Scenario: A popup with no watcher

- **WHEN** no watcher runs and a popup opens in a server
- **THEN** the popup's command asks that server to start the watcher by the action `watch`, and does not become the watcher

### Requirement: The watcher starts no helper for a source

The watcher SHALL read the identity of a source's command itself, at its next tick. It SHALL fork no process for it. No process that the watcher forked before a source's command started SHALL be a child of that command. (`nothing-detached@f807c7f5fcea`)

#### Scenario: Stopped as its source starts

- **WHEN** the watcher is stopped as soon as it has kept the pid of its source's command
- **THEN** once the watcher has ended, no process that it forked runs

#### Scenario: The source's children

- **WHEN** the source `sluice overview --json --watch --stdin` runs
- **THEN** the watcher left it no child, running or ended

### Requirement: A forecast is asked on the subscription

The watcher SHALL ask each forecast as a line `forecast <branch> <worktree>` on the input of the source that serves the worktree's repository. The source's reader SHALL keep the forecast that comes back, where the worktree's record still names that branch and head. The plugin SHALL start no `sluice` but its sources. (`one-subscription-per-plugin@57eb4dfbd366`)

#### Scenario: A worktree ahead of main

- **WHEN** a pane is in a worktree three commits ahead of `main`, and the source answers its forecast as clean
- **THEN** the pane's token says `3 ahead · clean`, the source's input got one `forecast main <worktree>` line, and no `sluice` but the source ran

## MODIFIED Requirements

### Requirement: One watcher per user on a host

The plugin SHALL run at most one watcher for all of a user's herdr
servers on a host. A lease in the plugin's state for the host SHALL
name the watcher's process. Any server's startup, pane or worktree
event, or a popup, SHALL have the watcher serve that server. A startup,
pane or worktree event's command SHALL become the watcher only when the
lease's process has ended or runs another plugin folder or version.

#### Scenario: Started twice

- **WHEN** the startup command runs in a server while a watcher holds
  the lease
- **THEN** no second watcher starts, the command exits 0, and the
  watcher serves that server

#### Scenario: Two servers

- **WHEN** two herdr servers on the host each run the startup command
- **THEN** one watcher runs, and it reads both servers' panes and sets
  tokens in both

#### Scenario: Watcher gone

- **WHEN** the lease's watcher has ended and a pane is created
- **THEN** the pane's event command becomes the watcher and takes the
  lease

#### Scenario: herdr stops

- **WHEN** every herdr server the watcher serves stops
- **THEN** within two ticks the watcher and its sources stop and the
  lease is released

#### Scenario: One of two servers stops

- **WHEN** the watcher serves two servers and the one that did not start
  it stops
- **THEN** the watcher and its sources keep running, and it calls only
  the server still running

#### Scenario: Another herdr server in its place

- **WHEN** a server stops and another starts at its socket's path and
  runs the startup command
- **THEN** the watcher serves the new server, and calls the new server
  for the panes and tokens the old one had

#### Scenario: A former version's watchers

- **WHEN** a former version's watchers run, one for each of two
  servers, and a server's event starts this version's watcher
- **THEN** within two ticks both former watchers and their sources have
  stopped, and one watcher serves both servers

### Requirement: The watcher only reads

The watcher SHALL learn the queue only from its sources, and SHALL ask its forecasts on their input. It SHALL NOT submit, withdraw, land or release anything.

#### Scenario: A blame

- **WHEN** a document shows `#9` blamed
- **THEN** the only sluice command that the watcher runs is its source
