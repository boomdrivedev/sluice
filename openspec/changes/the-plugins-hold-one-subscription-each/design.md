# Design

## Context

`sluice-host-answers-every-read` gave `sluice host` a subscription that serves many folders on one connection (its design D6), and a client of it, `sluice overview --json --watch --stdin` (its D7). A line `add <folder>` on the client's input adds a folder. A line `remove <folder>` removes it. Each line of output is one folder's newest overview, with `folder` in it. The client ends when its input closes and it serves no folder. Where folders stay, it goes on. A folder whose repository is gone, or has no sluice, gets the document that says so, and `sluice host` removes it.

The plugins do not use it yet:

- The herdr watcher's default source is `sluice overview --watch --json` with `each_repository = true` (`plugins/herdr/lib/sources.sh:8`). At each look, the watcher starts that source once in each repository that a pane of a server is in (`plugins/herdr/bin/watch.sh:614` to `640`). Each source is two processes: the command and its reader, `document.sh`. Each source is one subscription to `sluice host`.
- Before it starts a repository's source, the watcher runs `sluice overview --json` in the repository (`plugins/herdr/bin/watch.sh:635`). Where the overview holds no branch and no session, it marks the repository `none` and starts no source. It asks again at a look five minutes later, or after a nudge.
- `document.sh` gets the folder that its source ran in as `$2`, and runs `sluice status` there to learn a submission's worktree and its blamed tests (`plugins/herdr/bin/document.sh:84`).
- The board starts `sluice overview --json --watch` in the session's folder, one at a time, from the session's start for its life (`plugins/sluice-board/hooks/register.tsx:485` to `564`). The engine ends the child when the board's loop leaves it or the module unloads.

So the board holds one subscription, and the herdr watcher holds one for each repository that it shows.

## Goals / Non-Goals

**Goals:**

- The herdr watcher holds one subscription for all its repositories. (`one-subscription-per-plugin@ba0cdba772ca`)
- The board holds one subscription for its session, and a test proves it. (`one-subscription-per-plugin@ba0cdba772ca`)
- The watcher reads a repository's records only from its subscription. (`reads-go-through-the-resident@69134e5825d7`)

**Non-Goals:**

- A change of the `sluice` crate or of the protocol of `sluice host`. The client of D7 of `sluice-host-answers-every-read` is enough.
- A change of the one-shot reads `sluice status`, `sluice submit --check` and `sluice inbox`. They are reads that `sluice host` answers, and no subscription.
- A board that follows the session into another folder. The board follows the folder that the session started in, as today.

## Decisions

### D1. The herdr watcher runs one source for every repository

The default source is one command, run once: `sluice overview --json --watch --stdin`. The watcher names each repository to it as a folder on its input (D2). The watcher adds one folder for each repository: the top of a worktree of the repository that a pane is in, as the folder that the repository's source ran in today. Two folders of one repository share one reading in `sluice host`, so one folder is enough.

The source is still the command and its reader, two processes, the watcher's children (the plugin's D7). A source that ends is started again within one tick, as today. After it starts again, the watcher adds each repository that it holds again, since the new subscription holds no folder.

Before: one source for each repository, each its own subscription. Now: one source and one subscription for all of them. (`one-subscription-per-plugin@ba0cdba772ca`)

Not taken: a watcher that speaks the protocol of `sluice host` on its socket itself. The watcher is POSIX sh, and the client of `--stdin` already does this work.

### D2. The watcher names the folders on a named pipe that it holds open

The source's input is a named pipe beside its run file, `<run>.in`, made with `mkfifo` before the source starts. The source's subshell opens it for reading. The watcher then opens it for writing on a descriptor of its own, and holds it open while the source runs. So the source's input stays open between looks, and the subscription keeps its folders.

At each look, the watcher writes:

- `add <folder>` for each repository that a pane is in and that the source does not serve yet.
- `remove <folder>` for each repository that no pane is in, as `leave` stops a repository's source today. The watcher also forgets the repository's documents, as today.

The watcher keeps the folder that it added for each repository in a file beside the source's run file, so a later `remove` names the same folder.

Each write runs in a subshell. Where the source ended and the pipe has no reader, the write's signal ends that subshell and not the watcher. The watcher then marks the source ended, and the next tick starts it again (D1).

Example of the source's input over a session:

```text
add /w/sluice/feat
add /w/flume/main
remove /w/sluice/feat
```

Where the watcher ends, it ends its source, as today. Where the watcher is killed, the pipe's write end closes. The source still serves folders, so it goes on (D7 of `sluice-host-answers-every-read`). The next watcher ends it, as it ends each process that a dead watcher's pids file names today. (`nothing-detached@f807c7f5fcea`)

Not taken: a pipe from a long-lived writer process. It adds a process for each source and gives the same result.

### D3. Each line names its folder, and the reader reads it from the line

Each line of the source is one folder's overview with `folder` in it. `document.sh` keeps each document under its `host` and `repository`, from the document, as today. It runs `sluice status` in the line's `folder`, not in a folder that it was given at its start. A source of the user's that prints no `folder` runs nothing in a folder, as a source run once does today.

Before: the reader got its source's folder as `$2`. Now: each line names its folder.

### D4. The subscription tells which repositories have records, and the probe goes

The watcher runs no `sluice overview --json` before it adds a repository. It adds each repository that a pane is in. The first document of a folder tells what the probe told:

- A document that says that sluice is not set up (`"set_up": false`), or that the repository is gone (`"gone": true`): `sluice host` removes the folder from the subscription. The watcher marks the repository `none` with the time, keeps no document of it, and forgets its added folder. At a look five minutes after the mark, or after a nudge, it adds the repository again.
- A document that holds no branch and no session: the watcher marks the repository `none`, and keeps no document of it. The folder stays on the subscription. A later document of the folder that holds a branch or a session marks the repository `records`, and is kept.
- Any other document: the watcher marks the repository `records`, and keeps the document, as today.

The popup reads the mark as today: `none` gives `No sluice records in this repository.`

Before: one more read of each repository at each look that started its source. Now: no read but the subscription. A repository that gets its first submission shows it at the next document, and not five minutes later. (`reads-go-through-the-resident@69134e5825d7`)

### D5. A source of the user's names its folders with `folders = true`

In `sources.toml`, `folders = true` replaces `each_repository = true`. A source with `folders = true` runs once, and gets the `add` and `remove` lines on its input (D2). It prints `sluice/overview@1` lines, each with `folder`. A source without it runs once, with its input closed, as a source run once does today.

The default source, as the README shows it:

```toml
[[sources]]
name = "local"
command = ["sluice", "overview", "--json", "--watch", "--stdin"]
folders = true
```

A `sources.toml` that names `each_repository` is refused. The plugin's log names the line and says that `folders = true` replaces it, and the default source runs. This is what the plugin does today with a line outside the subset that the README states.

Before: `each_repository = true` ran the command once in each repository. Now: `folders = true` runs it once and names the repositories on its input. This breaks a `sources.toml` that names `each_repository`. `never-break-an-existing-client@b539ef8e2413` allows it before the first public release of sluice. The plugin keeps no reading of the old key.

The key's word is `folders`, the word of the subscription's `add <folder>` lines and of the document's `folder` field. (`names-use-the-domains-terms@343c9fac7fc5`)

### D6. A sluice with no `--stdin` is named

The source's standard error goes to a file beside its run file, `<run>.err`. When the source ends and its error holds `unexpected argument '--stdin'` or `unrecognized subcommand`, the watcher sets the plugin's problem:

```text
sluice 0.97.5 cannot serve the plugin's folders on one subscription: the plugin needs sluice 0.98.0
```

The first version is the first line of `sluice --version`. The second is the plugin's own version, from `herdr-plugin.toml`, which a release writes (`plugins-come-from-the-tools-flake@78305fd0bae0`). The popup shows the problem, as it shows the problem of a sluice with no overview today. The next source that prints a document clears it.

Before: the probe named a sluice with no `overview`. Now: the source's own error names a sluice with no `--stdin`.

### D7. The board keeps its one subscription, and a test proves it

The board does not change. It starts `sluice overview --json --watch` in the session's folder, one at a time. The engine ends the child when the board's loop leaves it, or the module unloads. With no `--stdin`, that watch is one subscription of one folder (D7 of `sluice-host-answers-every-read`).

A new requirement says it: at most one watch runs at any time over the session's life. A test counts the watches that run at one time while a watch ends and starts again, `/sluice` asks for one, the repository has no sluice and `/sluice` starts the next, and the session starts again in one load.

Not taken: a board that feeds `add` and `remove` lines to its watch. The engine's `process.spawn` writes its `input` once and closes it. The board serves one folder, so it needs no lines.

## Claims

Each claim that binds the plugins, and what this design does with it.

| Claim | This design | Where |
|---|---|---|
| `one-subscription-per-plugin@ba0cdba772ca` | Makes it hold. The herdr watcher holds one subscription for all its repositories. The board holds one for its session. | D1, D2, D7 |
| `reads-go-through-the-resident@69134e5825d7` | Keeps it. The watcher reads a repository's records only from its subscription. Each one-shot read goes to `sluice host`. | D4 |
| `clients-never-fetch-or-start-a-tool@8aa9c57f19b2` | Keeps it. The plugins run only sluice's commands, which reach `sluice host`. They run no `git fetch`. | D1, D7 |
| `one-read-in-flight@46e5af6765be` | Keeps it, in `sluice host`. One folder for each repository, and two folders of one repository share one reading. | D1 |
| `names-use-the-domains-terms@343c9fac7fc5` | Keeps it. The new key is `folders`, the subscription's word. `boom terms check` runs on each commit. | D5 |
| `never-break-an-existing-client@b539ef8e2413` | Keeps it. The change breaks a `sources.toml` that names `each_repository`, before the first public release of sluice. | D5 |
| `nothing-detached@f807c7f5fcea` | Keeps it. The source and its reader are the watcher's children. The watcher ends them, and a later watcher ends those of a killed watcher. | D2 |
| `a-leaked-child-fails-its-test@9a82a5147ba6` | Keeps it. The herdr tests run under `tests/leaks.sh`, and the board's tests under the engine, which ends each child with the module. | Tasks |
| `plugins-come-from-the-tools-flake@78305fd0bae0` | Keeps it. The problem of D6 names the plugin's own version, which is sluice's. | D6 |
| `each-part-is-a-moon-project@561ca40fb85c` | Does not touch it. The change adds no part. | — |
| `one-task-per-check@1c550fb50c1d` | Keeps it. Each test of the tasks runs in a task that the plugin's project has: `herdr:test-sh`, `herdr:test-dash` and `sluice-board:test`. | Tasks |
| `no-task-reads-markdown@a25dda6425d7` | Keeps it. No task checks the README's section that the change edits. | Task 3.1 |
| `the-gate-validates-specs-strictly@d8e057526e31` | Keeps it. `openspec validate --strict` runs before each commit. | Tasks |
| `open-changes-archive-cleanly@de24a2987568` | Keeps it. The change archives alone into a scratch copy of the specs. | Tasks |
| `a-decision-that-changes-a-rule-says-before-and-now@87e920a42421` | Keeps it. Each decision that changes what the plugin does says Before and Now. | D1 to D6 |
| `tools-at-their-pinned-versions@274efe1244ff` | Does not touch it. The tests run the sh, dash, jq and Claude Code that the tree pins. | — |
| `each-tracked-file-has-a-formatter@7bd720a7a6c1` | Does not touch it. The change adds no file type. | — |
| `residents-do-the-network-and-git-work@a2d68cf95ffe` | Does not touch it. The watcher reads a worktree's head from its files, as today. | — |
| `one-resident-per-tool@dc963165544c` | Does not touch it. | — |
| `one-github-client-per-host@786dccccffe1` | Does not touch it. | — |
| `residents-ask-each-other-by-socket@1c25d836b9e1` | Does not touch it. | — |
| `workers-end-with-their-owner@292e869158d6` | Does not touch it. | — |
| `a-build-takes-its-version-from-its-tree@b97cf1919925` | Does not touch it. | — |
| `a-release-tags-a-release-commit-off-main@c2c371868638` | Does not touch it. | — |
| `main-names-version-0-0-0@beed3874e15c` | Does not touch it. The versions in D6 are sample versions. | — |
| `no-credential-in-the-repository@a349a974ad13` | Does not touch it. | — |
| `nothing-commercial@80a056165cac` | Does not touch it. | — |
| `rule-details-break-then-not-then-source@c31fda3f2059` | Does not touch it. The change writes no rule. | — |
| `rules-name-rules-by-their-words@1b08e8dc3be7` | Does not touch it. The change writes no rule. | — |

## Risks / Trade-offs

- [One source for every repository is one point of failure. Where it ends, every repository's documents stop until the next tick.] → The watcher starts it again within one tick and adds each repository again. Each folder's first document comes at its addition, so no repository waits for a change.
- [A named pipe has no reader for a moment while the source starts again.] → The watcher opens the pipe for writing only after the source's subshell opened it for reading, and each write runs in a subshell.
- [A killed watcher leaves a source that still serves folders.] → The next watcher ends it, as it ends a killed watcher's sources today. Until then it holds one subscription, not one for each repository.
- [A user's source that read its folder from its working folder breaks.] → The plugin refuses `each_repository` and names `folders = true`. No public release of sluice has clients.

## Migration Plan

- A watcher of this version that starts ends a former version's watchers and their sources, as today. Its first look adds each repository on one source.
- The former version's per-repository run files go with the former watcher's state, as a former version's state goes today.
- A `sources.toml` that names `each_repository` gets the refusal of D5 in the plugin's log, and the default source runs until the user edits it.
