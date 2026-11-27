# Proposal

## Why

The herdr plugin of sluice breaks two accepted claims.

- `nothing-detached@f807c7f5fcea`: no process runs detached. The plugin's `--ensure` starts its watcher with `nohup` and ends (`plugins/herdr/bin/watch.sh:105`). So the watcher runs after the command that started it ends. Each source also forks a subshell that sleeps one second and then reads the identity of the source's command (`plugins/herdr/bin/watch.sh:463`). The watcher records no pid for that subshell. The subshell is forked before the source's command starts, so it becomes a child of `sluice overview --json --watch --stdin`, which never reaps it.
- `one-subscription-per-plugin@57eb4dfbd366`: each plugin holds one subscription to a resident. The plugin forecasts each worktree with a `sluice submit <branch> --check --json` of its own (`plugins/herdr/bin/forecast.sh:84`), a client in addition to its subscription.

herdr's documents of its plugins agree. A startup command is not a supervised daemon. A plugin stops each detached process before the command that started it ends.

This change closes these owed faults:

- "watch.sh can leave a forked subshell for up to a second when stopped".
- "`sluice overview --json --watch --stdin` starts `sluice host` as its own child when no host runs, and leaves one finished child unreaped": the finished child is the subshell of `watch.sh:463`.
- The forecast part of "Polling left on deneb". The plugin's forecasts move onto the subscription.

## What Changes

- The command that herdr starts, `watch.sh --ensure`, becomes the watcher where no watcher runs. It is a child of the herdr server that started it, for all of its life.
- When that herdr server ends, the watcher stops with its sources. Before it stops, it asks another server that it serves to start the next watcher, by the plugin's new action `watch`. That server starts the watcher as its own child.
- The popup's command only registers its server and wakes the watcher. Where no watcher runs, it asks its herdr server to start one by the action `watch`.
- The watcher reads the identity of a source's command itself, at its next tick. It forks no subshell for it.
- A subscription takes a line `{"forecast": "<worktree>", "branch": "<branch>"}`. `sluice host` answers it on the same connection with one line `{"forecast": {"folder", "branch", "forecast"}}`. The forecast is the document that `sluice submit <branch> --check --json` prints in that worktree, from the same read. The documents and the reports go on while the forecast is made.
- `sluice overview --json --watch --stdin` takes a line `forecast <branch> <worktree>` on its input, and prints the answer as `{"folder", "branch", "forecast"}`.
- The plugin's forecasts are asked on its source's input. The source's reader keeps each answer. The plugin starts no `sluice` but its source.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `herdr-watch`: the watcher is the command that herdr started, and ends with that herdr server. It forks no helper for a source. Its forecasts are asked on the subscription.
- `resident-subscriptions`: a subscription answers a forecast.

## Impact

- `crates/sluice/src/host_process/subscription.rs`: the ask `forecast`, the line `forecast`, and `Serves::forecast`. The forecasts in progress are held in a `FuturesUnordered` of futures-util.
- `crates/sluice/src/host_process/mod.rs`: `Subscribed::forecast` answers by the read of `sluice submit --check --json`.
- `crates/sluice/src/subscriber.rs`: the input line `forecast <branch> <worktree>` and its output line.
- `Cargo.toml`: the feature `alloc` of futures-util.
- `plugins/herdr/bin/watch.sh`, `bin/forecast.sh`, `bin/document.sh`, `bin/status.sh`, `lib/keep.jq`, `lib/common.sh`, `herdr-plugin.toml` and `README.md`.
- `plugins/herdr/tests/run.sh`, `tests/fake/sluice` and `tests/forecast-answer.sh`.
