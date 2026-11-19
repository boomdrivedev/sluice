## Context

See proposal.md, Why. The host process is `sluice host`
(`crates/sluice/src/host_process/mod.rs`): it takes `host/lock`, binds
its socket, sends its output to `host/log`, and its loop, `serve`, ends
it once nothing is in play, nothing follows it and no run holds room,
taking `host/start.lock` as it decides. It hands over two ways: to a
newer sluice's command, which sends `hand-over`, waits for the old pid
to end (`client.rs`, `HAND_OVER_WAIT`) and starts its own; and, for a
change of `[telemetry]`, to a successor it starts detached itself,
holding the start lock until a process of another pid answers (record
0107 as amended 2026-11-13). A kept `sluice run` already starts itself
afresh in place by `execve` (`hand_over.rs`, `replace`), keeping its pid.

Under a service manager, the process the manager started is the one it
keeps. A daemon that hands over as the host process does ends that
process: the manager starts it again, it finds a successor holding the
host's lock, and a `sluice host` that finds the lock held ends at once,
so it would restart in a loop. Every decision below follows from keeping
the manager's process the daemon.

Boomdrive keeps its runner the same way sluice's daemon needs keeping:
a launchd agent of nix-darwin's primary user, and on NixOS a systemd
user unit of a lingering user restarted by that user's activation
(boomdrivedev/boomdrive `nix/modules/darwin.nix`, `nixos.nix`,
`activate.sh`'s `keep`). This change copies it, so the family keeps a
tool's long-running process one way (CLAUDE.md, The family).

## Goals / Non-Goals

**Goals:**

- One host process for each user, daemon or not: the daemon is the host
  process with its idle end off, never a second kind of process.
- The service manager's process stays the daemon across every hand-over.
- Nothing changes for a host with no daemon.

**Non-Goals:**

- A list of repositories for the daemon, an option for its environment,
  or a log of its own: the index, the service's own options and
  `host/log` stand.
- A daemon on a host with neither nix-darwin nor NixOS kept by anything
  of sluice's: such a host keeps `sluice daemon` running its own way.

## Decisions

**D1. The daemon is the host process with its idle end off.**
`sluice daemon` runs `host_process::run` in a daemon mode: `serve`
never takes the idle end, and everything else, passes, looks, the room,
take-up, the log, the socket, is the host process's. `Named` gains
`daemon`, written to `host/version` and given in `hello`, and the
overview's `host_process` gives it. *Not taken:* a daemon that follows
the host process as `sluice run` does, keeping it from ending. The
manager's process would then be a follower, and the host process it
keeps would be a detached one the manager neither started nor stops.

**D2. A daemon takes the host from a host process that ends when idle,
and waits behind another daemon.** Where `host/lock` is held, the daemon
reads `host/version` and reaches the holder. A holder that is no daemon,
whatever its version, is asked `hand-over`, and waited for as long as
`HAND_OVER_WAIT`. The daemon then takes the lock, and exits 3 naming the
log where the holder still lives. A holder that is a daemon is asked
nothing: the daemon says once on stderr which daemon it waits for and
blocks on the lock. *Not taken:* ending at once, as `sluice host` does,
which under `KeepAlive` or `Restart=always` restarts in a loop while the
holder lives. *Not taken:* asking another daemon to hand over, which
makes two daemons trade the host back and forth.

**D3. A daemon hands over by starting afresh in place.** Asked by a
newer sluice's command, a daemon lets each task finish its round
(`stop_all`), flushes its telemetry, lets its lock and socket go, and
replaces its image by `execve` with `<the asker's executable> daemon`
and the environment it started with, through `hand_over.rs`'s `replace`
with no document handed. `Request::HandOver` gains `program`, the
asker's executable resolved through every link, `serde(default)`, so a
bare `hand-over` from an older command still reads, and a holder that is
no daemon ignores it. Where `program` is absent, the daemon starts
afresh as its own executable. *Not taken:* the daemon starting afresh as
its own executable, which a newer command finds older again and asks
again at every command. *Not taken:* a newer command using an older
daemon as it is, which holds every later request to what the older
process knows.

**D4. A command tells the daemon started afresh by its `since`.**
After asking a daemon to hand over, a command waits within
`HAND_OVER_WAIT` for a host process to answer that is not the image it
asked: another pid, or the same pid with another `since`. It uses that
process as a start of its own, whatever its version. Where the image it
asked still answers once the wait has passed, it fails naming the log.
`since` is written to the millisecond, so an image started afresh within
the second it started is told apart. *Not taken:* comparing the version
that answers with the command's own, which a test's fake older daemon
and a host's own build of one version cannot pass. *Not taken:* a field
of its own for the image, where the start time already says it.

**D5. A settings change a daemon cannot take up in place is a start
afresh in place.** `TakeUp`'s `HandOver(tables)` in daemon mode starts
afresh as the executable the daemon started as (`Successor::program`),
with `daemon` and the environment it started with, in place of starting
a detached successor (record 0107 as amended 2026-11-13). Where the
start fails, it takes its lock and socket back and goes on, said once,
as a successor that does not answer is today.

**D6. The start lock is carried across the start afresh.** Before its
`execve`, the daemon holds `host/start.lock`, clears close-on-exec on
that descriptor, and names its number in `SLUICE_START_LOCK_FD`. `main`
takes the variable out of the environment first, beside
`SLUICE_HANDED_OVER`, so no child inherits it. The image started afresh
adopts the descriptor and lets it go once its socket answers. A command
meanwhile waits on the start lock and reaches the daemon started afresh.
Where the start fails, the old image sets close-on-exec back and lets the
lock go once its socket answers again. *Not taken:* letting the lock go
at the `execve`. A command in that gap would start a host process of its
own, which D2 then has hand over, a round's worth of churn for each
hand-over.

**D7. A signalled daemon stops as a hand-over does.** `SIGTERM` and
`SIGINT` set the loop's `stopping`, which ends it through `stop_all`
within `tasks::FINISH_FOR`, its lock and socket let go, exit 0. The
services give it time: launchd's `ExitTimeOut` and systemd's
`TimeoutStopSec` are 75 seconds, `FINISH_FOR` and a margin. Runs are
their own process groups and outlive it (135).

**D8. A daemon started in a repository notes it in the index.** Before it
runs as the host process, `sluice daemon` notes its working folder's
repository where that has sluice, as every command does
(`host_process::ask::note`), and needs none to start. *Not taken:*
repositories named on its command line or in the module, which record
0093's "Index, on any sluice use" leaves out.

**D9. On nix-darwin, a launchd agent copied from Boomdrive's runner's.**
`launchd.user.agents.sluice`: `/bin/sh -c "/bin/wait4path /nix/store &&
exec <sluice> daemon"`, `RunAtLoad`, `KeepAlive`, `ThrottleInterval` 30,
`ProcessType` `Interactive`, `AbandonProcessGroup` (a stop ends the
daemon alone), `ExitTimeOut` 75, `EnvironmentVariables` `HOME` and the
profiles' `PATH`, then the system's folders, and `StandardOutPath` and
`StandardErrorPath` `~/Library/Logs/sluice-daemon.log`, which holds what
the daemon says before it holds the host and sends its output to
`host/log`. nix-darwin reloads the agent where its line changes, so a new
package restarts it.

**D10. On NixOS, a lingering user's unit restarted by the user's
activation, copied from Boomdrive's runner's.**
`systemd.user.services.sluice`: wanted by `default.target`,
`ConditionUser` the user, `enableDefaultPath = false` with the profiles'
`PATH`, `ExecStart` `<sluice> daemon`, `Restart=always`, `RestartSec=30s`,
`startLimitIntervalSec = 0`, `KillMode=process`, `TimeoutStopSec=75s`,
`restartIfChanged = false`; and `users.users.<user>.linger = true`, so
it runs from boot with no one logged in. NixOS restarts no changed user
unit, so `activate.sh` gains `keep <user> <program> <argument>...`,
Boomdrive's step for `sluice.service`, run in the existing `sluice` user
activation after the link where the daemon is on. *Not taken:* a system
unit with `User=`, which NixOS restarts by itself, but which runs apart
from the user's manager, where the set runs Boomdrive's runner for
sluice's user. The family's tools would then be kept two ways.

**D11. The service runs the module's sluice.** Its program is
`services.sluice.package`'s `bin/sluice`, so a new package changes the
line that D9 and D10 restart on. Where the package is null, the
launchd line runs `exec sluice daemon` from its `PATH`, and the unit's
`ExecStart` runs `sluice daemon` through the runtime shell, since
systemd looks a bare name up in its own folders and not the unit's
`PATH`.

**D12. One option.** `services.sluice.daemon.enable`, an enable option,
false. Its service is declared only with `services.sluice.enable`. A
host that moves sluice's state folder with `XDG_STATE_HOME` in its
users' shells gives the service the same variable through the service's
own options, `launchd.user.agents.sluice.serviceConfig.EnvironmentVariables`
or `systemd.user.services.sluice.environment`, so the daemon and the
user's commands reach one host process. The configuration page says so.

**D13. `sluice run`'s line names the daemon.** Its one line on stderr
says sluice starts its own host process, that nothing needs `sluice
run`, and that `sluice daemon` keeps it running on a machine that
chooses to. It stays hidden and a follower (record 0093 as amended
2026-11-13, decision 2).

**D14. Rules and records.** Rule core.51 admits the daemon's service
where the host turns it on, in its text and its details. core.31's
details move "a service, unit or list of repositories a host installs or
keeps for sluice" under "Not ruled out" for that service alone. Record
0098 is marked amended by record 0093's amendment of 2026-11-18. Record
0093 gains an amendment of what this build chose, D1 to D13, accepted as
the change's choices, as the amendment of 2026-11-13 was.

## Risks / Trade-offs

- [A newer command's executable is a build in a worktree that goes
  away.] → The daemon runs on from the image already loaded. A later
  start afresh for settings that names the gone executable fails and the
  daemon goes on, said once (D5). On NixOS the next activation restarts
  the unit's own command line (D10).
- [`since` to the millisecond changes the overview's `host_process.since`
  text.] → It stays RFC 3339, and nothing in sluice parses it but D4.
- [An older non-daemon host process reads `hand-over` with `program`.] →
  The request is internally tagged and its unit variant takes no fields;
  the build checks serde reads the extra field there, and a test sends
  it to a host process as an older command would.
- [`KillMode=process` leaves a stopped unit's runs in its control group.]
  → They are the runs that outlive the host process (135). systemd says
  they are left over at the next start, as it does for Boomdrive's
  runner.
- [Two daemons configured for one user.] → The second waits and says so
  once (D2). Nothing runs twice.

## Migration Plan

A host turns `services.sluice.daemon.enable` on and retires its own
standing `sluice run` unit in the same switch. A `sluice run` left
running follows the daemon and drives nothing. Turning the option off
removes the service. The host process then ends when idle again, as on
any host. Nothing in the records or the host's state changes.
