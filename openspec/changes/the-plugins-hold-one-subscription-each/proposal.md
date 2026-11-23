# Proposal

## Why

The herdr plugin's watcher starts one `sluice overview --watch --json` for each repository that a pane is in (`plugins/herdr/lib/sources.sh:8`, `plugins/herdr/bin/watch.sh:640`). Before it starts one, it runs a one-shot `sluice overview --json` in the repository, to find whether sluice has records there (`plugins/herdr/bin/watch.sh:635`). So a watcher that shows ten repositories holds ten subscriptions to `sluice host`, ten pairs of processes, and one more read for each repository at each look.

The claim `one-subscription-per-plugin@ba0cdba772ca` names this as what breaks it: "a plugin that starts a watcher for each folder that it shows, as `sluice overview --watch` does today". The change `sluice-host-answers-every-read` built the subscription that serves many folders on one connection, and its client `sluice overview --json --watch --stdin`. This change makes the plugins use it.

The board, the Claude Code plugin `sluice-board`, holds one `sluice overview --json --watch` for its session, one at a time (`plugins/sluice-board/hooks/register.tsx:489`, `feed`). It already holds one subscription. No test proves that it holds one over the session's life.

Claims that this change makes hold:

- `one-subscription-per-plugin@ba0cdba772ca`: the herdr watcher holds one subscription for all its repositories. The board holds one subscription for its session.

## What Changes

- The herdr watcher runs one source for all its repositories: `sluice overview --json --watch --stdin`. It writes `add <folder>` on the source's input for each repository that a pane is in, and `remove <folder>` when no pane is in the repository. It writes them on a named pipe that it holds open.
- When the source ends, the watcher starts it again within one tick, and adds each repository again.
- The reader of the source's lines reads each line's repository from the document, and runs `sluice status` in the line's `folder`.
- The watcher runs no `sluice overview --json` to find whether a repository has sluice's records. The first document of a folder on the subscription tells it. A repository with no sluice leaves the subscription, and the watcher adds it again at a look five minutes later, or after a nudge. A repository with sluice and no records stays on the subscription, and its document is not kept until it holds records.
- In `sources.toml`, the key `folders = true` replaces `each_repository = true`. A source with `folders = true` runs once and gets the `add` and `remove` lines on its input. The plugin refuses `each_repository`: its log names the key and the key that replaces it, and the default source runs.
- Where the host's sluice has no `--stdin`, the popup names the version of that sluice and the version that the plugin needs.
- The plugin's processes on a host are the watcher, its tick's sleep, the source's command and its reader: four, whatever the number of repositories.
- The board keeps its one subscription. A new requirement says that it holds at most one `sluice overview --json --watch` over the session's life, and a test proves it.

This change breaks a `sources.toml` that names `each_repository`. By `never-break-an-existing-client@b539ef8e2413`, a change may break a client before the first public release of sluice. The plugin keeps no reading of the old key.

Out of scope:

- The one-shot reads of the plugins: `sluice status`, `sluice submit --check` and `sluice inbox`. Each is a read that `sluice host` answers since `sluice-host-answers-every-read`, and none is a subscription.
- The `pass` that `sluice overview --json --watch --stdin` asks on a second connection where `sluice host` passes only when asked. That is a client of `sluice host` with no plugin in it, and no subscription.
- How the watcher starts and leaves its own process. The bolt's unit `a-service-owns-sluice-host-by-its-socket` and the claim `nothing-detached@f807c7f5fcea` govern the residents. The watcher's own start does not change here.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `herdr-watch`: one source with one subscription for every repository, the folders named on its input, no read to find a repository's records, the key `folders` in `sources.toml`, and four processes on a host.
- `plugin-board`: the board holds at most one subscription over the session's life.

## Impact

- `plugins/herdr/lib/sources.sh`: the default source, the key `folders`, and the refusal of `each_repository`.
- `plugins/herdr/bin/watch.sh`: `start` and `look` run one source with a named pipe as its input. `leave` writes `remove`. The probe goes. A source's standard error goes to a file beside its run file.
- `plugins/herdr/bin/document.sh`: the folder comes from the line.
- `plugins/herdr/bin/status.sh`: the popup's "no records" reads the mark that the watcher writes from the documents.
- `plugins/herdr/tests/run.sh` and `plugins/herdr/tests/fake/sluice`: the stand-in serves `overview --json --watch --stdin`, and the tests count one source.
- `plugins/herdr/README.md`: the section on sources.
- `plugins/sluice-board/tests/register.test.tsx`: a test of the board's one subscription.
- No change to the `sluice` crate or to the protocol of `sluice host`.
