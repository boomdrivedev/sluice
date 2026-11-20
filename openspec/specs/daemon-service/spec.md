# daemon-service Specification

## Purpose
The service sluice's own nix-darwin and NixOS modules declare to keep
`sluice daemon` running under the host's service manager, off unless the
host turns it on, since a machine runs a daemon by its own choice (135,
137; record 0093 as amended 2026-11-18; rule core.51).

## Requirements

### Requirement: The daemon's service is off unless the host turns it on

Each module SHALL declare `services.sluice.daemon.enable`, false unless
the host sets it. It SHALL be the one option that keeps `sluice daemon`
running, and with it unset the module SHALL add no service. It SHALL
take effect only with `services.sluice.enable` (record 0093 as amended
2026-11-18; rule core.51).

#### Scenario: Unset on nix-darwin

- **WHEN** the nix-darwin system of "The nix-darwin module" enables the
  module and leaves `services.sluice.daemon.enable` unset
- **THEN** `services.sluice.daemon.enable` is false, and
  `launchd.user.agents` names no `sluice` ("The daemon is off unless
  turned on" in `nix/tests/darwin.nix`)

#### Scenario: Unset on NixOS

- **WHEN** the NixOS system of "The NixOS module" enables the module and
  leaves `services.sluice.daemon.enable` unset
- **THEN** `systemd.user.services` names no `sluice`, and alice's
  `linger` is as it was ("The daemon is off unless turned on" in
  `nix/tests/nixos.nix`)

### Requirement: On nix-darwin the daemon is a launchd agent of the primary user

With the daemon on, the nix-darwin module SHALL declare
`launchd.user.agents.sluice`, which runs `sluice daemon` once the Nix
store is mounted, at load and kept alive, its restarts no more than
twice a minute, with the primary user's `HOME` and the profiles' `PATH`,
a stop ending the daemon alone, and what it says before it holds the
host in `~/Library/Logs/sluice-daemon.log`, as Boomdrive's runner's
agent is kept.

#### Scenario: The agent

- **WHEN** the nix-darwin system of "The nix-darwin module" enables the
  module and sets `services.sluice.daemon.enable = true`
- **THEN** `launchd.user.agents.sluice.serviceConfig` runs `/bin/sh -c
  "/bin/wait4path /nix/store && exec <package>/bin/sluice daemon"`, with
  `RunAtLoad`, `KeepAlive` and `AbandonProcessGroup` true,
  `ThrottleInterval` 30, `HOME` `/Users/alice`, and a `PATH` that begins
  `/etc/profiles/per-user/alice/bin` ("The daemon on nix-darwin" in
  `nix/tests/darwin.nix`)

### Requirement: On NixOS the daemon is a systemd user unit of the user who runs sluice

With the daemon on, the NixOS module SHALL declare
`systemd.user.services.sluice` for `services.sluice.user` alone, wanted
by the user's default target, and SHALL make that user linger, so the
daemon runs from boot with no one logged in. The unit SHALL run `sluice
daemon` with the profiles' `PATH`, restarted always and never given up
on, its stop ending the daemon alone, as Boomdrive's runner's unit is
kept.

#### Scenario: The unit

- **WHEN** the NixOS system of "The NixOS module" enables the module and
  sets `services.sluice.daemon.enable = true`
- **THEN** `systemd.user.services.sluice` runs `<package>/bin/sluice
  daemon` with `Restart` `always`, `KillMode` `process`,
  `ConditionUser` alice and no start limit, it is wanted by
  `default.target`, and `users.users.alice.linger` is true ("The daemon
  on NixOS" in `nix/tests/nixos.nix`)

### Requirement: On NixOS the user's activation restarts a changed daemon

Since NixOS restarts no changed user unit, the activation of the user
who runs sluice SHALL start the daemon's unit where it is not running,
restart it where the running daemon's command line is not the unit's,
and do nothing where they are one, waiting for no job and saying a line
only where it started or restarted it. Any other user's activation SHALL
do nothing.

#### Scenario: Not running

- **WHEN** `activate.sh keep alice <program> daemon` runs as alice, and
  the unit has no main process
- **THEN** it starts `sluice.service` without waiting, and says it
  started it ("keep: not running" in `nix/tests/activate.sh`)

#### Scenario: Running another command line

- **WHEN** it runs as alice, and the unit's main process runs another
  sluice's `daemon`
- **THEN** it restarts `sluice.service` without waiting, naming the
  command line that ran ("keep: another command line" in
  `nix/tests/activate.sh`)

#### Scenario: Running the unit's command line

- **WHEN** it runs as alice, and the unit's main process runs
  `<program> daemon`
- **THEN** it starts and restarts nothing and says nothing ("keep: the
  unit's command line" in `nix/tests/activate.sh`)

#### Scenario: Another user

- **WHEN** it runs as bob
- **THEN** it asks the service manager nothing and says nothing ("keep:
  another user" in `nix/tests/activate.sh`)

### Requirement: The daemon's service runs the module's sluice

The daemon's service SHALL run `services.sluice.package`'s sluice, so a
new package changes its command line and restarts it. Where the package
is null, it SHALL run the `sluice` the service's `PATH` finds.

#### Scenario: No package

- **WHEN** the nix-darwin system sets `services.sluice.package = null`
  and `services.sluice.daemon.enable = true`
- **THEN** the agent's line runs `exec sluice daemon` ("The daemon with
  no package" in `nix/tests/darwin.nix`)
