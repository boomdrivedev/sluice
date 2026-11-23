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

With the daemon on, the nix-darwin module SHALL declare `launchd.user.agents.sluice`. It SHALL run `sluice daemon` once the Nix store is mounted, at load and kept alive, with the user's `HOME` and the profiles' `PATH`. launchd SHALL end the job's process group when the job ends.

Before: a stop of the agent ended the daemon alone. Now: it ends the job's process group. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: The agent

- **WHEN** the nix-darwin system enables the module and sets `services.sluice.daemon.enable = true`
- **THEN** `launchd.user.agents.sluice.serviceConfig` runs `/bin/sh -c "/bin/wait4path /nix/store && exec <program> daemon"`, with `RunAtLoad` and `KeepAlive` true, `AbandonProcessGroup` false, `ThrottleInterval` 30, `HOME` `/Users/alice`, and a `PATH` that begins `/etc/profiles/per-user/alice/bin`

### Requirement: On NixOS the daemon is a systemd user unit of the user who runs sluice

With the daemon on, the NixOS module SHALL declare `systemd.user.services.sluice` for `services.sluice.user`, wanted by the default target, and SHALL make that user linger. The unit SHALL run `sluice daemon`, restarted always. Its stop SHALL end every process of the unit.

Before: the unit's stop ended the daemon alone. Now: it ends each process of the unit. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: The unit

- **WHEN** the NixOS system enables the module and sets `services.sluice.daemon.enable = true`
- **THEN** `systemd.user.services.sluice` runs `<program> daemon` with `Restart` `always`, `KillMode` `control-group`, `ConditionUser` alice and no start limit, it is wanted by `default.target`, and `users.users.alice.linger` is true

### Requirement: On NixOS the user's activation restarts a changed daemon

The activation of the user who runs sluice SHALL start the service where it does not run. Where the running host process runs another program than the profile's, the activation SHALL ask it to hand over and SHALL NOT restart the unit. It SHALL say a line only where it acted. Another user's activation SHALL do nothing.

Before: the activation restarted the unit, and the runs lost their owner. Now: it asks a hand-over. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: Not running

- **WHEN** the activation runs as alice, and the unit has no main process
- **THEN** it starts `sluice.service` without waiting, and says that it started it

#### Scenario: Running another command line

- **WHEN** it runs as alice, and the unit's main process runs another sluice's program
- **THEN** it asks that host process to hand over to the profile's program, restarts nothing, and names the program that ran

#### Scenario: Running the unit's command line

- **WHEN** it runs as alice, and the unit's main process runs the profile's program
- **THEN** it asks nothing and says nothing

#### Scenario: Another user

- **WHEN** it runs as bob
- **THEN** it asks nothing and says nothing

### Requirement: The daemon's service runs the module's sluice

The service SHALL run the system profile's `bin/sluice`, so a new package changes no service. The user's activation SHALL run its `sluice hand-over`, which has a host process of another sluice start afresh as it. With no package, the service SHALL run the `sluice` that its `PATH` finds.

Before: a new package restarted the daemon, and its runs lost their owner. Now: it is a hand-over, and the runs pass on. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: A new package

- **WHEN** a host process of the module's former package runs a combination's checks, and the system is switched to a new package
- **THEN** the service is not restarted, the host process answers with the new version and the same pid, and the checks continue

#### Scenario: No package

- **WHEN** the nix-darwin system sets `services.sluice.package = null` and `services.sluice.daemon.enable = true`
- **THEN** the agent's line runs `exec sluice daemon`
