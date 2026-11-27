# sluice for herdr

sluice's merge queue beside your agents in [herdr](https://herdr.dev).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="screenshots/popup-dark.png">
  <img src="screenshots/popup-light.png" alt="herdr with sluice's status popup over four Claude Code agents: two combinations under test at 3/5 with their tasks, one submission queued, a landing and a blame; each agent's line and the workspace's queue in the sidebar." width="800">
</picture>

```sh
herdr plugin install agentplot/sluice/plugins/herdr
```

Needs herdr 0.9.3 or later, the `sluice` of the same release and `jq` on
`PATH`, on Linux or macOS. It reads everything from one
`sluice overview --json --watch --stdin`, which it tells the folder of
each repository that a pane of any of your herdr servers is in, and shows
the repositories with sluice's records. One watcher does this for all of
your herdr servers on a host: the first server's startup command becomes
the watcher, a child of that herdr server, and each server's startup and
new panes and worktrees have it serve that server. When the herdr server
that started it stops, the watcher stops too, and another server it
serves starts the next one by the action `agentplot.sluice.watch`. The
watcher stops once no server runs. It never submits, withdraws or lands
anything.

## What you get

- **The status popup**, on the key you bind to `agentplot.sluice.status`:
  for the focused pane's repository, each branch's combinations under test
  with their tests and time, the submissions queued, and recent landings,
  blames and conflicts, under TESTING, QUEUED and RECENT, in the same words
  as the Claude Code board's `/sluice` pane. Each submission stands with
  its subject, or the branch it was submitted from where its submit gave
  none, beside its number, where the board shows its number alone for
  now. It redraws as the queue moves.
  A branch with nothing queued is left out, and counted on one line, once
  a quarter hour has passed since its last landing, blame or conflict.
  Branches come in the order the repository's `.config/sluice.toml` names
  them, as `sluice overview` gives them.
- **Checkouts held because their release failed**, under RELEASE FAILED, first
  under the branch's heading: each checkout, worktree and preview of the
  branch whose release failed and that sluice tries no more, with how many
  times it failed, the last line of what failed, and the command that
  releases it, in the Claude Code board's words:
  `✗ sluice/try/main/48 held · failed 4 times · usage: scripts/preview up`,
  then `sluice release sluice/try/main/48 · --accept-failure frees it
  without`. One another host made says `held on deneb`, and its command
  begins `on deneb:`. A branch with a held place is shown even with
  nothing queued. The popup shows the command; no row runs it.
- **Agents' checks**, under CHECKING in the popup: each worktree whose
  agent is running `sluice exec`, with how many of its tasks have ended
  (`fix-cards → main 3/8 ▓▓▓░░░░░░░ 1m10s`), and one that finished in the
  last quarter hour with what it is worth: `14 passed · reused when
  submitted` when the branch's gate takes those passes once that head is
  submitted, `not kept: uncommitted changes` when the worktree held more
  than its head. Under it, the host's runs: `host: 4 of 6 runs · 3 queue
  · 1 agent`. A check is load on the host, never a submission: it waits
  in no queue and lands nothing, but it takes one of the runs the queue's
  combinations are started under.
  `q` or Escape closes it, `j` jumps to the blamed submission's pane, `a`
  shows every repository the plugin holds, and the arrow keys and Page Up
  and Down scroll it when it is taller than the popup.
- **The dashboard**: the same view in its own tab, every repository at
  once (`agentplot.sluice.dashboard`).
- **Rows that act**: in the popup and the dashboard, Ctrl-click a
  submission to go to its agent's pane, or a failed test's `log of` row to
  open the log in a split. In the popup those rows are also numbered `[1]`
  to `[9]`: press the number. A link only acts on a worktree or a log the
  queue names, so a link printed by anything else does nothing.
- **Toasts**: one when a submission is blamed or conflicts, always; one
  when a submission lands, only when its agent's pane is open and not the
  one you are looking at.
- **Jump** (`agentplot.sluice.jump`): focus the pane whose worktree made
  the submission most recently blamed or in conflict. herdr's toasts take
  no click, so this is how a toast takes you to the pane.
- **A line on each agent's sidebar entry**, the `$sluice` token. herdr's
  Agents list shows only panes where it sees an agent running, so a plain
  shell's pane gets the token but no line:

  | The worktree | The line |
  |---|---|
  | has a submission queued or under test | `#95 testing 3m/15m` |
  | had its latest submission blamed, or in conflict | `#87 BLAMED · tests:t-cards`, `#53 CONFLICT · a.rs` |
  | is the pane's, and the pane has a branch named ([below](#naming-a-panes-branch)) | `release · 3 queued · #12 testing 3/8 · 2 checking` |
  | is running its checks with `sluice exec` | `checking 3/8`, or `checking 2m` for a command |
  | is ahead of its branch | `3 ahead · clean`, `3 ahead · conflicts #95 in builds.md +1` |
  | landed in the last 30 minutes | `#94 landed 12m` |
  | none of these | no line |

  The ahead line's forecast is asked on the same subscription, as a
  `forecast <branch> <worktree>` line on its input; the plugin starts no
  other `sluice` for it.
  A conflict names what it is with, a submission or the branch itself, and
  the first file in conflict, with how many more. The forecast runs again
  only when the worktree's head, the branch's revision or the submissions
  waiting on it change, not each time a combination starts or ends. On a
  branch with a `remote`, a forecast fetches the branch; set
  `ahead_check = false` (below) to show the count alone.
- **The queue on each workspace's row**, the `$sluice_q` token
  (`4 submissions · 1 held: release failed`, every branch counted, a
  checkout that several hosts list once).
- **The tab bar**: the focused repository's queue on one line, as the
  board's band says it, each submission with its subject or its branch:
  `sluice main · 3 submissions · testing main + #12 Batch review cards +
  #13 forecast 2m10s/15m · #14 Herdr popup waiting · last #9
  sidebar blamed on 4f1c2aa`. When
  agents' checks hold the queue back, the host has no run free while a
  submission is queued, and the line ends `· 1 slot to an agent's checks`.
  A branch with checkouts held because their release failed ends its line
  `· 1 held: release failed`, and gets one with nothing queued:
  `sluice main · 0 submissions · 1 held: release failed`.

Every token is set again within a minute and expires two minutes after,
so nothing stale stays if the plugin stops.

## Setting it up

Ask your agent to set up sluice's herdr plugin. With sluice's Claude Code
plugin installed, its setup skill reads your `config.toml`, binds the
popup and the jump to keys you have not bound, and adds the sidebar rows,
the tab bar's entry and toasts to the file as you keep it. It runs
`herdr config check` and `herdr server reload-config`, and says what it
added. Nothing of sluice's writes the file otherwise: the plugin changes
none of herdr's config.

Or add the lines yourself. Run the `setup` action
(`herdr plugin action invoke agentplot.sluice.setup`) to see the snippets
with this machine's paths filled in, and add them to herdr's `config.toml`,
or to whatever generates it, then run `herdr config check` and
`herdr server reload-config`. The keys, `prefix+m` for the queue and
`prefix+shift+m` for the jump, were chosen to avoid herdr's own default
keys; if you have bound them already, pick another free key.


```toml
[[keys.command]]
key = "prefix+m"
type = "plugin_action"
command = "agentplot.sluice.status"
description = "sluice: the queue"

[[keys.command]]
key = "prefix+shift+m"
type = "plugin_action"
command = "agentplot.sluice.jump"
description = "sluice: jump to the blamed submission's pane"

[ui.sidebar.agents]
rows = [
  ["state_icon", "machine", "workspace", "tab"],
  ["agent"],
  [{ token = "$sluice", rules = [{ contains = "BLAMED", fg = "#f55", bold = true }, { contains = "CONFLICT", fg = "#f55", bold = true }, { contains = "conflicts", fg = "#fc0" }] }],
]

[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace", "$sluice_q"],
  ["branch", "git_status"],
]

[ui]
tab_bar_right = [
  { type = "command", command = 'sh "$HOME/.local/state/herdr/plugins/agentplot.sluice/segment/bin/segment.sh"', interval_seconds = 5, timeout_seconds = 30 },
]

[ui.toast]
delivery = "herdr"
```

The sidebar rows above are herdr's own plus one; if you have rows of your
own, add only the `$sluice` row. Toasts need `[ui.toast]` delivery on. The
tab-bar command runs a copy the plugin keeps in its state directory, so it
survives a reinstall. herdr runs the command on the server the panes run
on, so a client on another host (`herdr --remote`, a saved machine, a
session bridging to the server) shows the server's queue. The entry is
the client's, though: its `config.toml` holds it, which is why the command
names `$HOME`. herdr runs it through `/bin/sh -lc`, which fills in the
server's `$HOME`, so one `config.toml` serves a Mac and a Linux box alike
wherever herdr keeps its state below the home directory, as it does by
default. `setup` prints the command for the host it runs on. herdr keeps
the line until a run ends and clears it when a run outlasts
`timeout_seconds`, so a limit of a few seconds blanks the line whenever
the server is loaded; at 30 a slow run is late, never blank.

## Naming a pane's branch

A pane's line follows the worktree it is in. When several agents share one
checkout, each working for a different branch whose submissions come from
other worktrees, the checkout can't say which branch is whose. Whatever
starts the agents can name it, with the pane token `sluice_branch`:

```sh
herdr pane report-metadata "$HERDR_PANE_ID" --source my-orchestrator \
  --token sluice_branch=release
```

The pane's line then shows that branch: `release · 3 queued · #12 testing
3/8 · 2 checking`, its submissions in no combination under test, its
oldest combination under test, and the agents' checks held to it;
`release · nothing queued`, or `release · no records` when sluice has no
records of it. The popup opened over the pane shows that branch first. A
submission, blame or conflict of the worktree's own still comes first, and
the pane gets no ahead forecast, so name a branch only where the worktree
doesn't say it.

Any source may set the token; let one own it on each pane. Reported
without `--ttl-ms`, herdr keeps it until it is replaced, cleared
(`--clear-token sluice_branch`) or the pane closes, but not across a
restart of herdr's server: report it again when the agent is restarted or
resumed. The plugin reads a server's panes, and the token with them, when
the server opens a pane or a worktree. It reads them again 15 seconds or
so later, and once a minute while the server shows a row of the plugin's.
A token reported as its pane opens shows within 15 seconds; one changed
later, with no event, within a minute, since a named pane always shows a
row. herdr gives every pane `HERDR_PANE_ID`; from elsewhere, use the id
`herdr pane list` shows. A Claude Code session's board reads the same from `SLUICE_BRANCH`
in the session's environment.

## Sources

By default the plugin runs `sluice overview --json --watch --stdin` once,
for every repository. It writes `add <folder>` on its input for each
repository that a pane of any herdr server is in, and `remove <folder>`
once no pane of any server is there. A repository where sluice is not set
up, or that has no records, shows `No sluice records in this repository.`
To read documents from anywhere else, list the commands in `sources.toml`,
in the plugin's config directory (`herdr plugin config-dir
agentplot.sluice`):

```toml
# Show the ahead count without forecasting.
ahead_check = false

# The default, kept: run once, told the folder of each repository a pane
# is in.
[[sources]]
name = "local"
command = ["sluice", "overview", "--json", "--watch", "--stdin"]
folders = true

# Any command printing sluice/overview@1 documents, one per line; run
# once, with its input closed.
[[sources]]
name = "build machine"
command = ["my-overview-stream", "app"]
```

A source with `folders = true` gets the `add` and `remove` lines on its
input, and prints each document with its `folder`. It also gets a line
`forecast <branch> <worktree>` for each ahead forecast, and prints the
forecast as `{"folder", "branch", "forecast"}`, `forecast` being what
`sluice submit <branch> --check --json` prints. A source that prints none
leaves the ahead count alone on the line. The key
`each_repository` is no key of the file: a file that names it runs no
source, and the plugin's log and the popup say
`sources.toml line <n>: each_repository is not a key: use folders = true`.

Each document is shown under its own `host` and `repository`; a repository
of the same name read from two hosts shows as two blocks. A pane is tied to
a submission by the worktree the submission came from, on this machine.

The file is read as a small part of TOML: `[[sources]]` tables, `key =
"string"`, arrays of strings on one line, `true` and `false`, and `#`
comments. Anything else is reported in the plugin's log with its line
number, and the default source is used.

## Tests

`plugins/herdr/tests/run.sh` runs every script against stand-in `herdr`
and `sluice` on scratch repositories; the repository's gate runs it as the
moon project `plugin-herdr`: `test-sh` under the system's sh, `test-dash`
under dash where it is installed, and `manifest`, `tests/manifest.sh`.
