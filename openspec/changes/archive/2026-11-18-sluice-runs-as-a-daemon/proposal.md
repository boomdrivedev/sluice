## Why

A machine that runs queues for other machines, and runs no sluice
command itself, has nothing to start sluice's host process: since sluice
0.97 the host process starts only from a command, and it ends once no
queue has work in play and nothing follows it (135). wldn's gate box
keeps a standing `sluice run` under a unit of its own for that.

Record 0093 as amended 2026-11-08, accepted by the user that day, names
the way such a machine runs: it keeps `sluice daemon` running, the same
host process, which never ends when idle (137, C.8). Record 0093 as
amended 2026-11-18 puts the service that keeps it running in sluice's
own nix modules, off unless the host turns it on. No `sluice daemon`
exists yet, and the modules install nothing for sluice to run (rule
core.51).

## What Changes

- **`sluice daemon`.** A command that runs as the user's sluice host
  process in the foreground and never ends for being idle. It moves
  every repository its index names, the work other machines write to
  their records among it, exactly as the host process does. sluice works
  as before with none running (135, 137).
- **The daemon takes the host's place.** Started while a host process
  that ends when idle holds the host, it has that process hand over and
  takes its place. Started while another daemon holds it, it waits for
  that one to end.
- **A daemon hands over in place.** A newer sluice's command, and a
  change of the host's settings no binding takes up in place, have a
  daemon start afresh in its own process, keeping its pid, so whatever
  keeps it running keeps the new one (135; record 0107). It holds the
  host's start lock across the start, so a command meanwhile reaches it.
- **A daemon stops when told.** Signalled to stop, it lets each task
  finish its round, lets its locks go and exits 0. The runs it started
  go on.
- **The overview names a daemon.** `host_process` gives `daemon`.
- **`sluice run` names `sluice daemon`.** Its one line on stderr says a
  machine that chooses to keep sluice running runs `sluice daemon`.
- **The modules keep the daemon running where the host turns it on.**
  `services.sluice.daemon.enable`, false by default: on nix-darwin a
  launchd agent of the primary user, on NixOS a systemd user unit of
  `services.sluice.user`, who lingers, restarted by the user's
  activation where its command changed. Each copies the way Boomdrive's
  runner's service is kept (record 0093 as amended 2026-11-18).
- **Rules and records.** Rule core.51 and core.31's details admit the
  daemon's service where the host turns it on. Record 0098 is marked
  amended. Record 0093 gains the build's choices.
- **Documents.** The README, the site's commands, configuration, guide
  and home pages, and the using-sluice skill say that a machine may keep
  `sluice daemon` running, and that nothing needs it.

## Capabilities

### New Capabilities

- `host-daemon`: `sluice daemon`, the host process kept running: how it
  takes the host's place, waits behind another daemon, hands over in
  place, and stops.
- `daemon-service`: the service sluice's nix modules declare to keep
  `sluice daemon` running on a host that turns it on.

### Modified Capabilities

- `host-process`: a second `sluice host` still ends at once, and a daemon
  takes its place; a daemon never ends for being idle; a newer command
  has an older daemon start afresh in place; the overview's
  `host_process` gives `daemon`.
- `settings-take-up`: a daemon's hand-over holds the start lock until it
  answers started afresh in place, keeping its pid.
- `host-driver`: `sluice run`'s line names `sluice daemon`.
- `host-settings-module`: the module adds no service but the daemon's,
  and that only where the host turns it on.

## Out of scope

- **Retiring `sluice run`.** It stays a hidden follower, so a unit that
  runs it keeps working. Taking it out waits for gate boxes to move
  their units to `services.sluice.daemon.enable`, a later change.
- **A machine whose signature a branch does not accept.** What it starts
  when it submits is record 0093's other unit queued on 2026-11-08, a
  change of its own.
- **Boomdrive's flake.** It imports sluice's module as today, so a host
  that takes sluice through the set turns the daemon on there. An option
  of the set's own is Boomdrive's change, if one is wanted.
- **A host's configuration.** A gate box turns the daemon on and retires
  its own `sluice run` unit in its own configuration.

## Impact

- `crates/sluice/src/lib.rs` and `crates/sluice/src/main.rs`: the
  `daemon` command; `sluice run`'s line.
- `crates/sluice/src/host_process/`: the idle end off for a daemon, the
  host's place taken or waited for, the in-place start afresh, the start
  lock carried across it, the stop on a signal, `daemon` in its `hello`.
- `crates/sluice/src/hand_over.rs`: the in-place start shared with a kept
  `sluice run`.
- `crates/sluice/src/overview.rs`: `host_process.daemon`.
- `nix/modules/`: `services.sluice.daemon.enable`, the launchd agent, the
  systemd user unit and the activation's `keep` step.
- Tests: `crates/sluice/tests/host_process.rs`, `instance.rs`,
  `peers.rs`, `cli.rs`; `nix/tests/darwin.nix`, `nixos.nix`,
  `activate.sh`.
- Rules and records: core.31, core.51; records 0093 and 0098.
- Documents: `README.md`; `site/src/content/docs/index.mdx`,
  `reference/commands.md`, `reference/configuration.md`,
  `guides/executors-and-systems.md`;
  `plugins/sluice/skills/using-sluice/`.
- Sources: clauses 129, 135, 137 and C.8 as amended 2026-11-08; records
  0093 (as amended 2026-11-08, 2026-11-13 and 2026-11-18), 0098, 0107;
  boomdrivedev/boomdrive's `nix/modules/darwin.nix`, `nixos.nix` and
  `activate.sh`'s `keep`.
