## Why

On deneb, sluice 0.97.5's host process ran `boomdrive forge-token` for
about one in three of its git calls, about seven process starts a second
while it fetched every repository's clone in turn. Each token the runner
mints is two GitHub API calls, so app 5719968's 5,000 calls an hour were
spent within about 25 minutes, and landings were refused for the rest of
each hour. Record 0108 holds a token in memory until five minutes before
its end, but each app held its own, and the host process opens one for
each binding it makes: one for each look at an idle repository, every
three seconds, and one for each repository's task. Each look's fetch
asked the command again.

## What Changes

- **The host process holds one token for each repository.** It makes one
  store of tokens as it starts and hands it to every binding it makes, on
  every thread: tasks, looks, the binding that answers a command's
  results, the one that names a repository in its metrics, and the check
  of settings taken up. No binding of its gets a repository's token while
  one it holds lasts (boomdrive-design's ruling of 2026-11-19, sent on by
  the user; record 0108 as amended 2026-11-19).
- **A repository's token is got under its own lock.** Two bindings that
  need it at once get it once. One repository's command never holds
  another repository's calls.
- **A renewal names the token GitHub refused.** Where another binding got
  a token since, that one is taken, and the command is neither told to
  erase nor asked.
- **Tokens are kept by app and repository.** Settings that name another
  app take none of the first app's tokens.
- **Recorded.** Record 0108's amendment of 2026-11-19, and rule core.29's
  details.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-identity`: the host process's bindings share one token for each
  repository, and a renewal takes a token another binding got since the
  refused one.

## Impact

- `crates/sluice/src/profiles/github/app.rs`: `Tokens`, the store, with a
  lock for each app and repository; `App::holding`; `App::issue` takes
  the refused token in place of `fresh`.
- `crates/sluice/src/identity.rs`, `crates/sluice/src/profiles/github/mod.rs`,
  `crates/sluice/src/profiles/pushed_refs.rs`: `Identity::renew` is given
  the refused password.
- `crates/sluice/src/profiles/mod.rs`: `Bound::open_holding`, and
  `profiles::Tokens`, so the host process names no forge (rule core.1).
- `crates/sluice/src/host_process/{mod,tasks,index}.rs`: the host
  process's store, handed to every binding it makes.
- Tests: `apps_holding_one_store_get_a_repositorys_token_once` and
  `a_token_got_afresh_is_the_one_another_refused_takes` in
  `crates/sluice/tests/profiles/forge.rs`;
  `the_host_process_buys_a_repositorys_token_once_across_its_looks` in
  `crates/sluice/tests/profiles/pull_requests.rs`.
- Records and rules: record 0108 (as amended 2026-11-19); rule core.29.
- Sources: clause 134 (as amended 2026-11-09), 135, C.8; records 0019,
  0093 and 0108; rules core.29 and core.31.

## Touches

```
crates/sluice/src/host_process/index.rs
crates/sluice/src/host_process/mod.rs
crates/sluice/src/host_process/tasks.rs
crates/sluice/src/identity.rs
crates/sluice/src/profiles/github/app.rs
crates/sluice/src/profiles/github/mod.rs
crates/sluice/src/profiles/mod.rs
crates/sluice/src/profiles/pushed_refs.rs
crates/sluice/tests/profiles/forge.rs
crates/sluice/tests/profiles/pull_requests.rs
crates/sluice/tests/pushed_refs.rs
docs/adr/0108-sluices-forge-token-may-come-from-a-credential-command-the-host-names.md
docs/architecture/core.md
openspec/changes/the-host-process-keeps-one-token-per-repository/**
```
