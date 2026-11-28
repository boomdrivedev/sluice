# Proposal

## Why

A sluice command starts `sluice host` where no host process answers. It does this also on a host where the service manager keeps `sluice daemon` running. On such a host, the service manager owns the host process, and a command must not start a second one beside it. The command also starts the process with code of its own, a `setsid` that it writes in the child before the program runs.

The reconcile of the narrative `the-residents` marks rows 5, 16 and 30 as a part break for sluice. This change closes them.

Claims that this change makes hold:

- `the-service-manager-starts-each-resident@d0e66e466414`: a client refuses where the service manager owns the resident, and starts it on a bare install.
- `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`: a client starts its own resident only on a bare install.
- `nothing-detached@f807c7f5fcea`: where the service manager keeps `sluice daemon`, no command starts a process that outlives it.

Claims that this change keeps holding: `an-idle-host-process-ends@dddbcaccd8db`, `sluices-runs-end-with-its-host-process@e814b716f3b1`.

## What Changes

- A new key of the host's settings, `[daemon] managed`. It says that the host's service manager keeps `sluice daemon` running for this user. It is false where unset.
- sluice's nix-darwin and NixOS modules write `[daemon] managed = true` where the host sets `services.sluice.daemon.enable`.
- Where `[daemon] managed` is true and no host process answers, a command starts no host process. It refuses at once. Its words name the socket, the setting and the host process's log. This includes the git hook, which then refuses the write, and a newer command whose older host process ended when asked to hand over.
- On a bare install, a command starts `sluice host` through process-wrap, in a session of its own, as before. process-wrap's `ProcessSession` replaces the command's own `setsid`. The command reaps the process through process-wrap's child.
- `sluice host` and `sluice daemon` read `[daemon] managed`, and refuse a value that is no boolean, as they refuse each key that they do not know.

Out of scope:

- A start of `sluice host` by socket activation. The unit `the-set-starts-each-resident-by-socket-activation` of the plan does it.
- The herdr plugin's watcher. Its source starts a subshell that sleeps one second in the background, and then runs `exec "$@"`. So the subshell becomes a child of `sluice overview`, which never reaps it. This is the cause of the owed fault "`sluice overview --json --watch --stdin` ... leaves one finished child unreaped". The unit `sluices-herdr-plugin-leaves-no-process-and-forecasts-from-its-subscription` of the plan changes that code.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `host-process`: a command starts the host process only where the service manager does not keep `sluice daemon`.
- `daemon-service`: with the daemon's service on, the module's settings file says that the service manager keeps `sluice daemon`.

## Impact

- `crates/sluice/src/host_process/client.rs`: the start through process-wrap, and the refusal where `[daemon] managed` is true.
- `Cargo.toml`: process-wrap, with its `std` and `process-session` features.
- `crates/sluice/src/instance.rs`: the `[daemon]` table, read by the host process, and the read of `[daemon] managed` alone for a command.
- `nix/modules/options.nix`: the option `services.sluice.settings.daemon.managed`, set where `services.sluice.daemon.enable` is set.
- The site's pages on the host's settings and on `sluice daemon`, record 0093, and rules core.31 and core.51.
