## ADDED Requirements

### Requirement: With the daemon's service on, the settings say that the service manager keeps the daemon

Each module SHALL declare `services.sluice.settings.daemon.managed`, a boolean. Where the host sets `services.sluice.enable` and `services.sluice.daemon.enable`, the module SHALL set it to true, as a default that the host can change (`the-service-manager-starts-each-resident@d0e66e466414`, rule core.51).

Before: the module's file did not say that the service manager keeps `sluice daemon`. Now: it says it with `[daemon] managed`.

#### Scenario: The daemon on

- **WHEN** a system enables the module and sets `services.sluice.daemon.enable = true`
- **THEN** the module's file holds `[daemon]` with `managed = true` ("The daemon says it is managed" in `nix/tests/darwin.nix` and `nix/tests/nixos.nix`)

#### Scenario: The daemon off

- **WHEN** a system enables the module and leaves `services.sluice.daemon.enable` unset
- **THEN** the module's file holds no `[daemon]` table
