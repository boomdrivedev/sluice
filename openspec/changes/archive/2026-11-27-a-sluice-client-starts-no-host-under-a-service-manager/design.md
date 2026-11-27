# Design

## Context

The gap, at main `fa12b1cd5`:

| Where | What the client does |
|---|---|
| `crates/sluice/src/host_process/client.rs:333` | `start` starts `sluice host` wherever no host process answers. It does not look at whether the service manager keeps `sluice daemon`. |
| `crates/sluice/src/host_process/client.rs:363` | The command writes its own `setsid` in a `pre_exec` step of the child. |
| `plugins/herdr/bin/watch.sh:463` | A source of the herdr plugin starts a subshell in the background and then runs `exec "$@"`. The subshell becomes a child of `sluice overview`, which never reaps it. This change leaves it to the unit that changes the watcher. |

flume does the same work in `crates/flume/src/served.rs`: its client starts no `flume host`, and says that the host's service manager runs it. sluice keeps the start on a bare install, which `the-service-manager-starts-each-resident@d0e66e466414` allows. Where a service manager keeps `sluice daemon`, sluice refuses as flume refuses.

## Goals / Non-Goals

**Goals:**

- Where the service manager keeps `sluice daemon`, no command starts a host process (`the-service-manager-starts-each-resident@d0e66e466414`, `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`).
- A command starts `sluice host` through process-wrap, which the accepted decision `each-worker-starts-through-process-wrap@025fae162990` names, and writes no `setsid` of its own.
- A host process that fails at its start is still said at once (record 0093 as amended 2026-11-24, point 7).

**Non-Goals:**

- Socket activation of `sluice host`.
- A wait for a daemon that the service manager starts again.

## Decisions

### D1. A setting says that the service manager keeps the daemon

The host's settings get the table `[daemon]` with one key, `managed`, a boolean, false where unset. `managed = true` says that the host's service manager keeps `sluice daemon` running for this user.

sluice's own modules write it. `nix/modules/options.nix` declares `services.sluice.settings.daemon.managed`. Where the host sets `services.sluice.enable` and `services.sluice.daemon.enable`, the module sets it to true, as a default that the host can change. A host that keeps `sluice daemon` running by another service sets the key by hand.

Before: no setting said whether a service manager keeps `sluice daemon`. Now: `[daemon] managed` says it, and the module writes it with the daemon's service.

*Not taken:*

- A look at the service manager itself from the command. It names a service manager outside a profile (core.1), and costs a process at each start.
- A file that the daemon writes in the state folder. The file stays where the person turns the service off, and then each command refuses.
- A variable in the environment. A person's shell and an agent's session do not get the service's environment.

### D2. Where the daemon is managed, a command refuses at once

`Client::start` reads `[daemon] managed` first, before it takes the start lock. Where it is true, the command starts nothing and fails with these words:

`sluice's host process answers on no socket at <socket>; the host's service manager runs sluice daemon ([daemon] managed in <settings>); see <log>`

Each way to `start` gets the refusal. These are a command with no host process that answers, the git hook, and a newer command whose older host process ended at a hand-over. A host process that answers is reached as before, a daemon or not.

The command does not wait for the service manager to start the daemon again. flume's client does not wait either.

*Not taken:* a wait of ten seconds for the socket. It makes each command on a host whose daemon is down wait, and the service manager's own wait between starts is longer.

### D3. A command reads [daemon] managed alone

The command reads the settings file from the same place that the host process reads it: `$XDG_CONFIG_HOME/sluice/config.toml`, else `~/.config/sluice/config.toml`, in the command's own environment. It reads the key `managed` of the table `[daemon]` and passes over each other table and key, as `Instance::guarding_from` does for `[forge]`.

A file that is not there gives false. A file that the command cannot read, or that is no TOML, also gives false. The command then starts `sluice host`, which refuses the file in its own words, and the command says those words at once (D4).

`sluice host` and `sluice daemon` read `[daemon]` with the rest of the file, each unknown key refused. A take-up of a changed file needs nothing for `[daemon]`, since the host process does not act on it.

### D4. A command starts the host process through process-wrap

On a bare install, the command starts `sluice host` with process-wrap's `StdCommandWrap` and its `ProcessSession` wrapper. The process runs in a session of its own, as before: no signal to the command's terminal or process group reaches it. The command keeps the rest of the start: `/` as the working folder, no input, its output in `host/log`, and the host's own environment.

While the command waits for `sluice host` to answer, it looks at the child through process-wrap's `try_wait`. A child that ended with a failure is said at once, with what the log holds since the start. Once the process answers, a thread of the command waits on the child through process-wrap. So the command reaps the process if it ends while the command runs.

Before: the command wrote `setsid` in a `pre_exec` step of its own. Now: process-wrap's `ProcessSession` makes the session.

*Not taken:*

- A double fork, so that `sluice host` is no child of the command. No established crate spawns a program that way. The code would be fork, pipe and exit steps of sluice's own.
- process-wrap's `KillOnDrop`. The host process outlives the command that starts it on a bare install (`an-idle-host-process-ends@dddbcaccd8db`).
