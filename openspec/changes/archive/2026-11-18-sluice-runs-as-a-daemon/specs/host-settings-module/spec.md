## MODIFIED Requirements

### Requirement: The module installs the package and nothing else

With `services.sluice.enable`, the module SHALL put
`services.sluice.package` in the system profile, by default the
flake's package for the host's system, which a host MAY replace or set
to null to install none. The module SHALL add no
launchd agent or daemon, no systemd unit, no file under `/etc`, no user
and no list of repositories, but the daemon's where the host turns it
on (daemon-service). Beside the link it SHALL write no other tool's
file (135; principle 10; rules core.31, core.51).

#### Scenario: The flake's sluice

- **WHEN** the nix-darwin system of "The nix-darwin module" enables the
  module and sets no package
- **THEN** its system profile holds the flake's sluice for
  aarch64-darwin ("The flake's sluice" in `nix/tests/darwin.nix`)

#### Scenario: A host's own sluice

- **WHEN** it sets `services.sluice.package` to a package of its own
- **THEN** the system profile holds that package and not the flake's,
  and the activation's line is unchanged ("A host's own sluice" in
  `nix/tests/darwin.nix`)

#### Scenario: No package

- **WHEN** it sets `services.sluice.package = null`
- **THEN** the system profile holds no sluice, and the activation still
  holds the link's line ("No package" in `nix/tests/darwin.nix`)

#### Scenario: Nothing else on nix-darwin

- **WHEN** the nix-darwin system with the module enabled is compared with
  the same system with it not enabled
- **THEN** the names under `environment.etc`, `launchd.agents`,
  `launchd.daemons`, `launchd.user.agents` and `users.users` are equal,
  and the activation, with the link's line taken out, is equal ("Nothing
  else on the host" in `nix/tests/darwin.nix`)

#### Scenario: Nothing else on NixOS

- **WHEN** the NixOS system with the module enabled is compared with the
  same system with it not enabled
- **THEN** the names under `environment.etc`, `systemd.services` and
  `systemd.user.services` are equal, each user's `linger` is equal, and
  the user activations' names differ by `sluice` alone ("Nothing else on
  the host" in `nix/tests/nixos.nix`)

#### Scenario: Only the daemon's service with the daemon on

- **WHEN** the nix-darwin system with the module and the daemon enabled
  is compared with the same system with the module alone enabled
- **THEN** `launchd.user.agents` differs by `sluice` alone, and the names
  under `environment.etc`, `launchd.agents`, `launchd.daemons` and
  `users.users` are equal ("Only the daemon's service" in
  `nix/tests/darwin.nix`)
