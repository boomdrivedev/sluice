# Design

## Context

The herdr plugin keeps one watcher for each user on a host (record 0094). Until this change, any herdr server's startup or event started the watcher with `nohup` when none ran. Its forecasts ran `sluice submit <branch> --check --json` for each worktree that moved. The accepted claims `nothing-detached@f807c7f5fcea` and `one-subscription-per-plugin@57eb4dfbd366` forbid both. herdr 0.9.5 runs a plugin's commands with no wait for their end. It logs each end. Its documents say that a startup command is no supervised daemon. They also say that a plugin stops a detached process before the command that started it ends.

## Decisions

### D1. The watcher is the command that herdr started

`watch.sh --ensure` registers its server. Where a watcher of this plugin folder and version holds the lease, the command wakes it and exits, as before. Where none does, the command becomes the watcher. It writes its output to `watch.log`, and herdr's server stays its parent for all of its life.

The watcher keeps the pid of its parent, and tests it with the shell's `kill -0` at each tick and each wake, with no fork. When the parent has ended, the watcher stops its sources and lets go of the lease. Then it asks another server that it serves to start the next watcher. It uses `herdr plugin action invoke watch --plugin agentplot.sluice`, the new action `watch` of the manifest, and stops at the first server that answers. herdr starts that command as the child of that server, and the command becomes the next watcher. So no watcher outlives the herdr server that started it, and a host with servers keeps a watcher.

A popup is no place for the watcher: its pane closes. `watch.sh --register`, the popup's, registers and wakes the watcher. Where none runs, it asks its server to start one by the action `watch`.

### D2. A forecast goes on the subscription

`sluice host` answers a forecast on the subscription that asks it. The ask is `{"forecast": "<worktree>", "branch": "<branch>"}`. The answer is `{"forecast": {"folder", "branch", "forecast"}}`, where `forecast` is the document that `sluice submit <branch> --check --json` prints in the worktree. `sluice host` makes it by the same read, so it joins the same reads in flight (`one-read-in-flight@e99acda774d0`). The connection's loop holds the forecasts in progress in one `FuturesUnordered` of futures-util. So a slow forecast holds back no document, no report and no other forecast.

The plugin still chooses what to forecast and when. `forecast.sh` reads the documents and the heads as before. It writes the worktree's record with the count alone, and prints one ask for each forecast due. The watcher writes `forecast <branch> <worktree>` on the input of the source that serves the worktree's repository. The source's reader keeps the answer on the record where the record still names that branch and head, and the tokens are set again. Where no source serves the repository, the record is removed, so the next forecast asks again.

### D3. The watcher reads a source's identity itself

The source's command is kept by its pid as it starts, as before. At its next tick, the watcher reads the command's identity (`lib/procs.sh`) and keeps it. The subshell that slept one second and read the identity goes. It was forked before the command started, so it was the command's child: `sluice overview --json --watch --stdin` held it unreaped after it ended.

## Risks

- A herdr older than 0.9.5 that cannot invoke an action leaves a host with no watcher until its next server event. The watcher says so in its log.
- A source of the user's that does not read `forecast` lines answers none. Its worktrees show the count ahead alone, as with `ahead_check = false`.
