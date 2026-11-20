# host-settings-module Specification

## Purpose
sluice's flake gives a nix-darwin and a NixOS module that writes the
host's own settings, `~/.config/sluice/config.toml`, from options named
as the file names them, holds no key, writes no other tool's file and
installs nothing for sluice to run (35, 71, 74, 78, 87, 135, 136;
record 0098; rule core.51).

## Requirements

### Requirement: sluice's flake gives a module for nix-darwin and for NixOS

sluice's flake SHALL give `darwinModules.sluice` and
`nixosModules.sluice`, each also `default`, declaring
`services.sluice.enable`, `services.sluice.package` and
`services.sluice.settings`, and `services.sluice.user` on NixOS. Each
SHALL evaluate in a system of its platform with the module enabled and
no other flake's module (agentplot/boomdrive record 0023; record 0098;
rule core.51).

#### Scenario: The nix-darwin module

- **WHEN** an aarch64-darwin system whose primary user is `alice`, home
  `/Users/alice`, imports `darwinModules.default` and sets
  `services.sluice.enable = true`
- **THEN** the system evaluates to a derivation ("The nix-darwin module"
  in `nix/tests/typecheck.nix`)

#### Scenario: The NixOS module

- **WHEN** an x86_64-linux system with the normal user `alice` imports
  `nixosModules.default` and sets `services.sluice.enable = true` and
  `services.sluice.user = "alice"`
- **THEN** the system's toplevel evaluates to a derivation ("The NixOS
  module" in `nix/tests/typecheck.nix`)

### Requirement: The flake gives three systems

The flake SHALL give its package, the module's default, for
aarch64-darwin, aarch64-linux and x86_64-linux and for no other system,
each evaluating to a derivation (record 0098).

#### Scenario: Every system's package

- **WHEN** the flake's `packages` are evaluated
- **THEN** it names aarch64-darwin, aarch64-linux and x86_64-linux and no
  other system, and each one's `default` evaluates to a derivation
  ("Every system's package" in `nix/tests/typecheck.nix`)

### Requirement: The settings are the host's own file under its own names

`services.sluice.settings` SHALL render the text of the host's own
settings file, each key named as the file names it. The module SHALL
declare every key sluice reads, under `runs`, `forge`, `signing`,
`peers`, `deliveries`, `caches` and `telemetry`, with its TOML type and
no default, so a key left unset is left out of the file and sluice's
own default holds. A key the module does not declare SHALL pass to the
file as written (35, 51, 71, 74, 78, 87, 136, 151; records 0098, 0102,
0106).

#### Scenario: Every key the module declares

- **WHEN** the module is enabled with every key it declares set but
  `forge.credential`, which stands in place of `forge.key`:
  - `runs` with `max = 3`, `cores = 12`, `reserved = 1` and two
    systems;
  - `forge` with `app_id`, `key = "~/.config/sluice/app.pem"` and
    `discovery = "60s"`;
  - `signing` with `signer` and `key = "~/.config/sluice/signing"`;
  - `peers` with `lease`, `alive` and `name`;
  - `deliveries` with `expire_after = "72h"`;
  - two `caches`, one with `dir`, `socket` and `env`, one with `dir`
    and `env`;
  - `telemetry` with `send = true`,
    `collector = "http://localhost:4318"`, `service = "sluice"`,
    `traces = false`, `metrics = true`, `logs = "tail"`,
    `logs_tail_bytes = 4096` and `attributes` holding
    `"deployment.environment" = "prod"`
- **THEN** the file is byte for byte `nix/tests/files/every.toml`
  ("Every key" in `nix/tests/file.nix`)

#### Scenario: A key the module does not declare

- **WHEN** the module is enabled with `settings.runs.later = "soon"`
  beside `runs.max = 1`
- **THEN** the file's `[runs]` holds `later = "soon"` and then
  `max = 1` ("A key the module does not declare" in
  `nix/tests/file.nix`)

### Requirement: The file's form

The file SHALL open with one line saying the module wrote it from
`services.sluice.settings`, then its keys before its tables, each sorted
by name. A value that is not a string, a boolean, an integer, a list of
strings, a table or a list of tables SHALL fail evaluation, naming its
option (record 0098).

#### Scenario: No settings

- **WHEN** the module is enabled with `settings = { }`
- **THEN** the file is the header line alone, byte for byte
  `nix/tests/files/empty.toml` ("No settings" in `nix/tests/file.nix`)

#### Scenario: A value the file cannot hold

- **WHEN** the module is enabled with `settings.runs.share = 1.5`
- **THEN** evaluating the file fails, naming
  `services.sluice.settings.runs.share` ("A value the file cannot hold"
  in `nix/tests/file.nix`)

### Requirement: sluice reads the module's file as written

The file the module renders with every key it declares set SHALL be
read by sluice's own reader of the host's settings, which refuses a
field it does not know, as the settings name it: every key of `runs`,
`forge`, `signing`, `peers`, `deliveries`, `caches` and `telemetry`
(35, 51, 71, 74, 78, 87, 136, 151).

#### Scenario: Reading the module's file

- **WHEN** sluice's reader reads `nix/tests/files/every.toml` on a host
  of sixteen cores, with a home of the test's own holding
  `.config/sluice/app.pem` and `.config/sluice/signing`, each of mode
  `0600`
- **THEN** it gives everything the file holds, as written
  (`the_modules_file_is_read_as_written` in
  `crates/sluice/src/instance.rs`):
  - a bound of 3, 12 cores, a reserve of 1 and the two systems;
  - the forge's app, its key at the home and a discovery of sixty
    seconds;
  - the signer and its key at the home;
  - the peers' lease, sign of life and name;
  - a span of 72 hours;
  - both caches, with their directories at the home and the socket and
    each variable as written;
  - telemetry sent to `http://localhost:4318` as `sluice`, traces off,
    metrics on, logs at `tail` and at most 4096 bytes, and the one
    attribute.

### Requirement: A key is named by its path, outside the store

`services.sluice.settings.forge.key` and
`services.sluice.settings.signing.key` SHALL each be a path that begins
with `/` or `~/` and lies outside the Nix store directory. A value
otherwise, or a Nix path value, SHALL fail evaluation, naming the
option. The failure SHALL say that the store is read by every user. The
module SHALL copy no key file into the store (78, C.4 pushed refs;
record 0098; rule core.51).

#### Scenario: A forge key in the store

- **WHEN** the module is enabled with `settings.forge.key` a path under
  the store directory
- **THEN** evaluation fails, naming `services.sluice.settings.forge.key`
  and the store ("A forge key in the store" in `nix/tests/file.nix`)

#### Scenario: A signing key in the store

- **WHEN** the module is enabled with `settings.signing.key` a path
  under the store directory
- **THEN** evaluation fails, naming
  `services.sluice.settings.signing.key` and the store ("A signing key in
  the store" in `nix/tests/file.nix`)

#### Scenario: A key's text in place of its path

- **WHEN** the module is enabled with `settings.signing.key` set to
  `-----BEGIN OPENSSH PRIVATE KEY-----`
- **THEN** evaluation fails, naming
  `services.sluice.settings.signing.key` ("A key's text" in
  `nix/tests/file.nix`)

#### Scenario: A key given as a Nix path

- **WHEN** the module is enabled with `settings.forge.key` a Nix path
  value
- **THEN** evaluation fails, naming `services.sluice.settings.forge.key`
  ("A key given as a Nix path" in `nix/tests/file.nix`)

### Requirement: The file is linked at sluice's path for the user who runs sluice

The module SHALL link `~/.config/sluice/config.toml` of the user who runs
sluice to the file it renders: on nix-darwin the system's primary user,
in the system's activation; on NixOS `services.sluice.user`, which has
no default, in that user's activation alone. A failed link SHALL stop
nothing else of the system's activation (rule core.51).

#### Scenario: On nix-darwin

- **WHEN** the nix-darwin system of "The nix-darwin module" enables the
  module with "Every key"'s settings
- **THEN** its activation holds
  `sudo --user=alice -- /usr/bin/env HOME=/Users/alice /bin/sh <activate.sh> link <file> || true`,
  where `<file>` is the store path of the text "Every key" renders
  ("On nix-darwin" in `nix/tests/darwin.nix`)

#### Scenario: A primary user with no home

- **WHEN** a nix-darwin system enables the module and its primary user
  has no `users.users.<user>.home`
- **THEN** evaluation fails, naming that option ("A primary user with no
  home" in `nix/tests/darwin.nix`)

#### Scenario: On NixOS

- **WHEN** the NixOS system of "The NixOS module" enables the module with
  "Every key"'s settings
- **THEN** its user activation `sluice` holds
  `if [ "$(id -un)" = alice ]; then <shell> <activate.sh> link <file> || true; fi`,
  where `<file>` is the store path of the text "Every key" renders
  ("On NixOS" in `nix/tests/nixos.nix`)

#### Scenario: A user the system does not declare

- **WHEN** a NixOS system enables the module with
  `services.sluice.user = "carol"` and declares no `carol`
- **THEN** evaluation fails, naming `services.sluice.user` and
  `users.users.carol` ("A user the system does not declare" in
  `nix/tests/nixos.nix`)

### Requirement: What is at sluice's path already

A link at `~/.config/sluice/config.toml` SHALL be replaced. A file or
folder there that is no link SHALL be moved once to
`config.toml.before-nix`, both paths named; where that path is taken
already, the activation SHALL change neither, name both and fail. It
SHALL say a line only where it moved or linked something, and SHALL link
the file alone, writing nothing else in `~/.config/sluice/` (rule
core.51).

#### Scenario: Nothing there

- **WHEN** `activate.sh link <file>` runs in a home with no
  `.config/sluice`
- **THEN** `.config/sluice/config.toml` links to `<file>`, and one line
  names both ("Nothing there" in `nix/tests/activate.sh`)

#### Scenario: A link there

- **WHEN** `.config/sluice/config.toml` links to another file, and
  `activate.sh link <file>` runs
- **THEN** it links to `<file>`; run again, it says nothing ("A link
  there" in `nix/tests/activate.sh`)

#### Scenario: A file there

- **WHEN** `.config/sluice/config.toml` is a file and
  `.config/sluice/app.pem` is beside it, and `activate.sh link <file>`
  runs
- **THEN** the file is at `.config/sluice/config.toml.before-nix`, the
  path links to `<file>`, both are named, and `app.pem` is as it was ("A
  file there" in `nix/tests/activate.sh`)

#### Scenario: A file there and one aside already

- **WHEN** `.config/sluice/config.toml` and
  `.config/sluice/config.toml.before-nix` are both files, and
  `activate.sh link <file>` runs
- **THEN** both are as they were, both are named, and it exits 1 ("A file
  there and one aside already" in `nix/tests/activate.sh`)

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

### Requirement: A module imported twice is taken once

`darwinModules.sluice` and `nixosModules.sluice`, and each `default`,
SHALL carry the key `github:agentplot/sluice#<platform>Modules.sluice`,
`darwin` or `nixos`, as Flume's flake keys its own, so a system that
imports the module twice, itself and through another flake's module,
declares `services.sluice` once (agentplot/boomdrive record 0023;
record 0098; rule core.51).

#### Scenario: The nix-darwin module imported twice

- **WHEN** an aarch64-darwin system whose primary user is `alice`, home
  `/Users/alice`, imports `darwinModules.default`, sets
  `services.sluice.enable = true`, and imports `darwinModules.sluice`
  again under another module
- **THEN** `darwinModules.sluice` and `darwinModules.default` each carry
  the key `github:agentplot/sluice#darwinModules.sluice`, the system
  evaluates to a derivation, `services.sluice.enable` is declared once,
  and the activation links the file ("The module imported twice on
  nix-darwin" in `nix/tests/darwin.nix`)

#### Scenario: The NixOS module imported twice

- **WHEN** an x86_64-linux system with the normal user `alice` imports
  `nixosModules.default`, sets `services.sluice.enable = true` and
  `services.sluice.user = "alice"`, and imports `nixosModules.sluice`
  again under another module
- **THEN** `nixosModules.sluice` and `nixosModules.default` each carry
  the key `github:agentplot/sluice#nixosModules.sluice`, the system's
  toplevel evaluates to a derivation, `services.sluice.enable` is
  declared once, and alice's user activation links the file ("The
  module imported twice on NixOS" in `nix/tests/nixos.nix`)

### Requirement: The module renders the forge's credential command

The module SHALL declare `forge.credential`, which sluice reads in place
of `forge.key`, and render it in the file as a list of strings, the
command's program and then each argument, in their order (record 0108).

#### Scenario: A forge credential command

- **WHEN** the module is enabled with `forge` holding `app_id = 123456`,
  `credential = [ "/opt/forge/bin/token-helper" "--app" "sluice" ]` and
  `discovery = "60s"`, and nothing else set
- **THEN** the file is byte for byte `nix/tests/files/credential.toml`,
  its `[forge]` holding `app_id`, then `credential` as a list of the
  three strings in their order, then `discovery` ("A forge credential
  command" in `nix/tests/file.nix`)

### Requirement: sluice reads the module's credential command as written

The file the module renders with `forge.credential` set SHALL be read by
sluice's own reader of the host's settings as the settings name it: the
forge's app, the credential command's program and each argument in their
order, no key, and the discovery (51, 136; record 0108).

#### Scenario: Reading the module's credential command

- **WHEN** sluice's reader reads `nix/tests/files/credential.toml`
- **THEN** it gives the forge's app 123456, the credential command
  `/opt/forge/bin/token-helper` with the arguments `--app` and `sluice`
  in their order and no key, and a discovery of sixty seconds
  (`the_modules_credential_file_is_read_as_written` in
  `crates/sluice/src/instance.rs`)

### Requirement: A forge credential is a list of strings

`services.sluice.settings.forge.credential` SHALL be a list of strings,
the command's program and then each argument, and a string in its place
SHALL fail evaluation by the option's type, naming the option. Whether
`key` and `credential` are both named is sluice's to refuse at its
read, not the module's (records 0098, 0108; rule core.51).

#### Scenario: A command written as a shell line

- **WHEN** the module is enabled with `settings.forge.credential` set to
  the string `"token-helper --app sluice"`
- **THEN** evaluation fails, naming
  `services.sluice.settings.forge.credential` ("A credential given as a
  string" in `nix/tests/file.nix`)

### Requirement: A forge credential holds no token's text

An element of `services.sluice.settings.forge.credential` that begins
`ghs_`, `ghp_`, `gho_`, `ghu_`, `ghr_` or `github_pat_` SHALL fail
evaluation, naming the option and saying the file is in the Nix store,
which every user reads. A program in the store SHALL pass, since a
program is no secret (rule core.51).

#### Scenario: A token's text in the command

- **WHEN** the module is enabled with `settings.forge.credential` set to
  `[ "/opt/forge/bin/token-helper" "ghs_notarealtoken" ]`
- **THEN** evaluation fails, naming
  `services.sluice.settings.forge.credential` and the store ("A token's
  text in the credential" in `nix/tests/file.nix`)

### Requirement: Each telemetry option says its unit and values

Each option under `services.sluice.settings.telemetry` SHALL say in its
description what the key does, its values or its unit and range, and
sluice's default where it is unset (principle 13; record 0106). The
descriptions of `send` and `collector` SHALL say that `send = true`
needs a `collector`, which sluice refuses a sending table without
(record 0106 as amended).

#### Scenario: The telemetry options' descriptions

- **WHEN** the module's options are evaluated
- **THEN** the description of `settings.telemetry.logs` holds `off`,
  `summary` and `tail`; that of `settings.telemetry.logs_tail_bytes`
  holds `bytes` and `65536`; that of `settings.telemetry.collector`
  holds `http://` and `send = true`; and that of
  `settings.telemetry.send` holds `false` and `collector`
  ("The telemetry options say their units and values" in
  `nix/tests/file.nix`)
