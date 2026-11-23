# Proposal

## Why

The herdr plugin's watcher starts one `sluice overview --watch --json` for each repository that a pane is in (`plugins/herdr/lib/sources.sh:8`, `plugins/herdr/bin/watch.sh:640`). Before it starts one, it runs a one-shot `sluice overview --json` in the repository, to find whether sluice has records there (`plugins/herdr/bin/watch.sh:635`). So a watcher that shows ten repositories holds ten subscriptions to `sluice host`, ten pairs of processes, and one more read for each repository at each look.

The claim `one-subscription-per-plugin@ba0cdba772ca` names this as what breaks it: "a plugin that starts a watcher for each folder that it shows, as `sluice overview --watch` does today". The change `sluice-host-answers-every-read` built the subscription that serves many folders on one connection, and its client `sluice overview --json --watch --stdin`. This change makes the plugins use it.

The watcher's reader also starts `sluice status <id> --json` for a new blame or conflict whose worktree or failed tests it did not learn (`plugins/herdr/bin/document.sh:84`). The overview document does not carry them. A recent line names a submission by its `id`, `recent_from` gives its worktree's name and not its folder, and a `blamed` line gives no failed test. So a process starts for each such event.

The board, the Claude Code plugin `sluice-board`, holds one `sluice overview --json --watch` for its session, one at a time (`plugins/sluice-board/hooks/register.tsx:489`, `feed`). It already holds one subscription. No test proves that it holds one over the session's life.

Claims that this change makes hold:

- `one-subscription-per-plugin@ba0cdba772ca`: the herdr watcher holds one subscription for all its repositories. The board holds one subscription for its session.

## What Changes

- The herdr watcher runs one source for all its repositories: `sluice overview --json --watch --stdin`. It writes `add <folder>` on the source's input for each repository that a pane is in, and `remove <folder>` when no pane is in the repository. It writes them on a named pipe that it holds open.
- When the source ends, the watcher starts it again within one tick, and adds each repository again.
- The watcher runs no `sluice overview --json` to find whether a repository has sluice's records. The first document of a folder tells it.
- The overview document gives two new fields for each branch: `recent_paths`, the folder of each live submission that the recent lines name, and `recent_failed`, the failed tests of each blamed submission that they name. The reader reads a submission's folder and tests from them, and runs no `sluice status`.
- In `sources.toml`, the key `folders = true` replaces `each_repository = true`. A file that names `each_repository` is refused with an error that names `folders = true`, and the watcher runs no source from it.
- The plugin's processes on a host are the watcher, its tick's sleep, the source's command and its reader: four, whatever the number of repositories.
- The board keeps its one subscription. A new requirement says that it holds at most one `sluice overview --json --watch` over the session's life, and a test proves it.

The user approved these changes on 2026-11-23 (commit `2f2197b6e`).

This change breaks a `sources.toml` that names `each_repository`. By `never-break-an-existing-client@b539ef8e2413`, a change may break a client before the first public release of sluice. The plugin assumes the sluice of its own release (`plugins-come-from-the-tools-flake@78305fd0bae0`), and has no code for an older sluice.

Out of scope:

- A board that follows a session that moves to another repository. Decided by the user on 2026-11-23: the board follows the folder that the session started in.
- The one-shot reads `sluice submit --check` and `sluice inbox`, and the board's `sluice status` when a person opens a blamed row. Each is a read that `sluice host` answers, and none starts while the plugin is idle.
- The `pass` that `sluice overview --json --watch --stdin` asks on a second connection where `sluice host` passes only when asked. That is a client of `sluice host` with no plugin in it, and no subscription.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `herdr-watch`: one source with one subscription for every repository, the folders named on its input, no read to find a repository's records, no `sluice status` from the reader, the key `folders` in `sources.toml`, and four processes on a host.
- `overview`: each branch gives `recent_paths` and `recent_failed`.
- `plugin-board`: the board holds at most one subscription over the session's life.

## Impact

- `crates/sluice-core/src/overview.rs`: `BranchView` gets `recent_paths` and `recent_failed`, read from the live records, as `recent_from` is.
- `plugins/herdr/lib/sources.sh`: the default source, the key `folders`, and the refusal of `each_repository`.
- `plugins/herdr/bin/watch.sh`: `start` and `look` run one source with a named pipe as its input. `leave` writes `remove`. The probe goes.
- `plugins/herdr/bin/document.sh` and `plugins/herdr/lib/keep.jq`: the folder and the failed tests come from the document. `learn` goes.
- `plugins/herdr/bin/status.sh`: the popup's "no records" reads the mark that the watcher writes from the documents.
- `plugins/herdr/tests/run.sh` and `plugins/herdr/tests/fake/sluice`: the stand-in serves `overview --json --watch --stdin`, and the tests count one source.
- `plugins/herdr/README.md`: the section on sources.
- `plugins/sluice-board/tests/register.test.tsx`: a test of the board's one subscription.
- No change to the protocol of `sluice host`.
