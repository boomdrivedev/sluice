## Why

sluice writes every commit as `sluice <sluice@localhost>`, a name fixed
in the code (`NAME` and `EMAIL` in `profiles/local_refs.rs`, the
environment of `git commit-tree` in `profiles/remote_refs.rs`). Where
sluice lands as a GitHub App, GitHub records the push by the App but
links the landing commit to no account, so no landing shows the App
that made it. Boomdrive's record 0007 counts "GitHub shows the
product's name on every landing" among the consequences of its one App.

Record 0019 as amended 2026-11-18, point 1, decides it: where a branch
is on a forge sluice reaches as a GitHub App, every commit sluice
writes for it, a landing's and a records commit alike, has the App's
own bot account as its author and committer, the App's slug followed by
`[bot]` at `<bot user id>+<slug>[bot]@users.noreply.github.com`, read
from GitHub with the App's own credential and kept on the host. A
branch with no App keeps `sluice <sluice@localhost>`.

## What Changes

- **A commit names the identity that pushes it.** A landing commit on a
  branch sluice pushes to GitHub as the App, and a records commit on a
  remote it pushes to as the App, name the App's bot as author and
  committer. Every other commit keeps `sluice <sluice@localhost>`.
- **A combination's commit stays sluice's.** It keeps sluice's name and
  the time 0 on every host, so one combination forms one commit
  wherever it is formed (core.6, core.17, core.58).
- **The identity names its author.** sluice's identity interface gains
  the author a commit pushed as it names; the GitHub profile implements
  it, and the pushed refs profile asks it, naming no forge (core.1,
  core.2).
- **The App reads its bot and keeps it on the host.** Its login is read
  as the pull requests' author is, `GET /app` under a key and GraphQL's
  `viewer` under a credential command; its user id is
  `GET /users/<login>`. Both are kept in the host's state for every
  process and read again a day after, the kept ones standing while
  that read fails.
- **A read that fails writes nothing.** Where the host keeps no bot,
  the commit that needed it is not written, and the round says why in
  GitHub's words.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-identity`: a commit sluice pushes as the App names the App's
  bot, read from GitHub and kept on the host; a combination, and a
  commit pushed otherwise, keep sluice's name.

## Out of scope

- **`sluice forge check` reading commit statuses** (record 0019 as
  amended 2026-11-18, point 3). It is a change of its own; nothing here
  waits on it, and nothing here reports the author in the check.
- **One App for every organization a host lands for** (point 2).
  sluice already buys each repository's token from that repository's
  own installation, so nothing is built for it.
- **The pull requests' author.** `App::bot`, which tells the requests
  sluice opened, is read as before.
- **weir-branch's and timber's commits.** They are written on their own
  refs under their own names by their own tools, which sluice neither
  writes nor pushes.

## Impact

- `crates/sluice/src/identity.rs`: an `Author`, and `Identity::author`.
- `crates/sluice/src/profiles/github/`: `App::author`, the `Identity`
  implementation, and the author kept under `forge/authors/` in
  `kept.rs`.
- `crates/sluice/src/profiles/local_refs.rs`, `pushed_refs.rs` and
  `remote_refs.rs`: each commit written by the author its remote is
  pushed as.
- The branch seam, `crates/sluice-seams/src/lib.rs`: `Branch::commit`
  takes the branch it writes for, which the core's landings in
  `crates/sluice-core/src/step.rs` give it and the router in
  `crates/sluice/src/profiles/branches.rs` routes the landing by; each
  other implementer takes it as it is given.
- Tests: `tests/profiles/forge.rs` and its fake, `tests/profiles/forge/github.rs`,
  `tests/local_refs.rs`, `tests/pushed_refs.rs`, `tests/remote_refs.rs`,
  `tests/profiles/pull_requests.rs`, `tests/profiles/conformance.rs`.
- Site: `reference/state.md` and `guides/github-app.md` say who a
  landing names.
- Records and rules: record 0019 as amended 2026-11-18, with the
  build's choices; rule core.43.

## Touches

```
crates/sluice-seams/src/lib.rs
crates/sluice-core/src/step.rs
crates/sluice-core/src/ops.rs
crates/sluice-fakes/src/lib.rs
crates/sluice/src/identity.rs
crates/sluice/src/profiles/branches.rs
crates/sluice/src/profiles/github/pulls.rs
crates/sluice/src/profiles/github/app.rs
crates/sluice/src/profiles/github/kept.rs
crates/sluice/src/profiles/github/mod.rs
crates/sluice/src/profiles/local_refs.rs
crates/sluice/src/profiles/pushed_refs.rs
crates/sluice/src/profiles/remote_refs.rs
crates/sluice/tests/profiles/forge.rs
crates/sluice/tests/profiles/forge/github.rs
crates/sluice/tests/profiles/pull_requests.rs
crates/sluice/tests/profiles/conformance.rs
crates/sluice/tests/local_refs.rs
crates/sluice/tests/pushed_refs.rs
crates/sluice/tests/remote_refs.rs
site/src/content/docs/reference/state.md
site/src/content/docs/guides/github-app.md
docs/adr/0019-sluice-pushes-as-a-github-app-whose-token-reaches-only-its-own-git.md
openspec/changes/every-merge-names-the-app-as-its-author/**
```
