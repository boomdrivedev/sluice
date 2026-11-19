## Why

On deneb, sluice 0.97.5's host process asked every repository's remote
every three seconds, idle repositories among them: its look at each
repository it held no task for fetched the remote's records at every
scan, and a watched repository's task fetched them at every pass, a
second apart. Each fetch from GitHub was made as app 5719968, so idle
repositories alone spent its allowance. Clause 129, as boomdrive-design
amended it on 2026-11-19 on the user's word to fix deneb's host process
now (blueprints 8fbd5cc), has a repository with nothing in play ask its
remote once a minute at most, and a submit, a landing or a watch's
opening ask it at once.

## What Changes

- **Where nothing is in play, the remote is asked once a minute at
  most.** Whether a reading of a repository asks its remote is decided
  as it begins, from the repository's reading marks, which the host
  process's look and the repository's task share: it asks where none has
  asked yet, where the last reading found work in play, where the last
  ask began a minute ago or more, and where a command told of a change,
  a watch opened or a forecast asked afresh since. Otherwise it reads
  every remote as the last ask fetched it.
- **The look is taken only where an ask is due.** The host process still
  goes through its index every three seconds, and looks only at a
  repository whose remote is due.
- **A one-shot read stays current within its tick.** Between asks, it is
  answered from a look that asks nothing, which keeps the last ask's
  refusal.
- **Every reading asks where passes are taken only when asked** (D14),
  each a command's ask.
- **Recorded.** Record 0093's amendment of 2026-11-19, rule core.31's
  rule and details, and the reference's one-shot reads.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `reader-tick`: a remote's records are asked for once a minute at most
  where nothing is in play, and at once on a submit, a command's change,
  a watch's opening and a forecast's second ask.

## Impact

- `crates/sluice/src/host_process/tasks.rs`: `Marks::asks`, the reading
  marks' last ask, last finding and the ask due; `Tasks::due`; a look
  taken only where due; a pass told whether it asks.
- `crates/sluice/src/host_process/pass.rs`: `pass` asks each remote only
  where told to.
- `crates/sluice/src/host_process/index.rs`: the look holds its binding
  where it asks nothing.
- `crates/sluice/src/host_process/mod.rs`: `IDLE_ASK`.
- Tests: `an_idle_repositorys_remote_is_asked_once_a_minute` and
  `a_tasks_pass_asks_where_work_is_in_play_or_a_watch_opens` in
  `crates/sluice/src/host_process/tasks.rs`;
  `an_idle_repositorys_remote_is_asked_once_a_minute` in
  `crates/sluice/tests/host_process.rs`; `a_daemon_lands_what_another_host_submits`
  in `crates/sluice/tests/peers.rs` submits before the daemon starts.
- Records and rules: record 0093 (as amended 2026-11-19); rule core.31.
- Sources: clause 129 (as amended 2026-11-19, boomdrivedev/blueprints
  8fbd5cc), 135, 136; records 0093 and 0108; rules core.25, core.31 and
  core.32.

## Touches

```
crates/sluice/src/host_process/index.rs
crates/sluice/src/host_process/mod.rs
crates/sluice/src/host_process/pass.rs
crates/sluice/src/host_process/tasks.rs
crates/sluice/tests/host_process.rs
crates/sluice/tests/peers.rs
crates/sluice/tests/support/mod.rs
docs/adr/0093-a-hosts-queues-move-in-sluices-own-host-process-started-when-work-arrives-and-pull-requests-are-queued-on-the-forge.md
docs/architecture/core.md
openspec/changes/an-idle-repositorys-remote-is-asked-once-a-minute/**
site/src/content/docs/reference/commands.md
```
