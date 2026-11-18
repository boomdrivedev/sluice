---
title: The herdr plugin
description: sluice's plugin for herdr, the terminal multiplexer for coding agents.
sidebar:
  order: -1
---

[herdr](https://herdr.dev) runs coding agents in panes. sluice's herdr
plugin shows each repository's queue beside them: what is being tested, what
is waiting, and what landed or was blamed. Each submission is tied to the
pane of the agent that made it. The plugin only reads; it never submits,
withdraws or lands anything.

<a class="screenshot-light" href="/sluice/screenshots/popup-light.webp"><img src="/sluice/screenshots/popup-light.webp" alt="herdr with the status popup over four Claude Code agents: two combinations under test at 3/5 with their tasks, one submission queued, a landing and a blame; each agent's line and the workspace's queue in the sidebar." width="1408" height="856" /></a>
<a class="screenshot-dark" href="/sluice/screenshots/popup-dark.webp"><img src="/sluice/screenshots/popup-dark.webp" alt="herdr with the status popup over four Claude Code agents: two combinations under test at 3/5 with their tasks, one submission queued, a landing and a blame; each agent's line and the workspace's queue in the sidebar." width="1408" height="856" /></a>

## Install

```sh
herdr plugin install boomdrivedev/sluice/plugins/herdr
```

It needs herdr 0.9.3 or later, sluice 0.12 or later and `jq` on `PATH`, on
Linux or macOS. It reads what it shows from `sluice overview --json
--watch`, the same document the
[Claude Code plugin](/sluice/guides/claude-code/) reads, run once in each
repository with sluice's records that a pane of any of your herdr servers
is in. One watcher does this for all of your herdr servers on a host. The
first server's startup starts it, and each server's startup and new panes
and worktrees have it serve that server. It stops once none of them runs.

## Set up your keys and sidebar

Ask your agent to set up sluice's herdr plugin. With sluice's Claude Code
plugin installed, its setup skill reads your `config.toml`. It binds the
status popup and the jump to keys you have not bound, and adds the
sidebar rows, the tab bar's queue and toasts to the file as you keep it.
It runs `herdr config check` and `herdr server reload-config`, and says
what it added. Nothing of sluice's writes the file otherwise.

### By hand

The plugin changes none of herdr's config. To add its lines yourself,
run its setup action:

```sh
herdr plugin action invoke agentplot.sluice.setup
```

It shows the snippets to add to herdr's `config.toml`, or to whatever
generates it, with your paths filled in. They bind `prefix+m` to the
status popup and a key to the jump. They add a row under each agent in the
sidebar, the queue on each workspace's row and the queue in the tab bar,
and turn `[ui.toast]` on for toasts. Then run `herdr config check` and
`herdr server reload-config`.

The tab bar's entry names a copy of the plugin's segment script from
`$HOME`:

```toml
[ui]
tab_bar_right = [
  { type = "command", command = 'sh "$HOME/.local/state/herdr/plugins/agentplot.sluice/segment/bin/segment.sh"', interval_seconds = 5, timeout_seconds = 30 },
]
```

herdr runs the command through `/bin/sh -lc`, which fills in each host's
home directory. So one `config.toml` serves a Mac and a Linux box alike,
wherever herdr keeps its state below the home directory, as it does by
default. The setup action prints the command for the host it runs on.
herdr clears the tab bar's queue when a run outlasts `timeout_seconds`,
so at 30 a slow run on a loaded host is late, never blank.

## The status popup

`prefix+m` opens the queue of the focused pane's repository. Under TESTING
are each branch's combinations under test with their tests and elapsed
time; under QUEUED, the submissions with their states; under RECENT, the
latest landings, blames and conflicts. The words are those of the Claude
Code plugin's `/sluice` pane, and the popup redraws as the queue moves.
Each submission stands with its subject, or the branch it was submitted
from where its submit gave none, beside its number: `#14 Herdr popup`,
`#13 forecast`. The `/sluice` pane shows its number alone for now.
The tab bar names them the same way.
A branch with nothing queued is left out once a quarter hour has passed
since its last landing, blame or conflict; one line counts the branches
left out. Branches come in the order the repository's
`.config/sluice.toml` names them.

When the task runner says how a combination's tasks are going, as moon
does, the combination shows how many have ended and a bar, and its tasks
are listed beneath its tests:

```
    ▶ testing main + #12 Batch review cards + #13 forecast  3/5 ▓▓▓▓▓▓░░░░  2m10s / 15m
        #check-fast #check-tools · sandbox
        ✓ sluice:test      1m48s
        ✓ site:prose    replayed
        ✓ sluice:tools     2m30s
        … sluice:e2e     running
        · core:test      waiting
```

The tab bar and the sidebar show the count and its ten-cell bar alone.

## Checkouts whose release failed

When a checkout's release keeps failing, sluice stops trying and holds the
checkout until you release it
([held checkouts](/sluice/reference/commands/#checkouts-held-because-their-release-failed)).
The popup shows each one first under its branch's heading, under RELEASE
FAILED, with the worktrees and previews of that branch held the same way:

```
main at b7d2e94 · 0 submissions · 0 combinations under test (up to 2 at once)
  RELEASE FAILED
    ✗ sluice/try/main/48 held · failed 4 times · usage: scripts/preview up
        sluice release sluice/try/main/48 · --accept-failure frees it without
    ✗ worktree unit-a held · failed once · rm: cannot remove .stack
        sluice release --worktree /work/sluice/unit-a · --accept-failure frees it without
```

Each row gives the last line of what failed; `sluice status <branch>` has
all of it. The command beneath runs the release again, and with
`--accept-failure` frees it without running the release. A checkout
another host made says `held on deneb`, and its command begins `on
deneb:`, since only that host can release it. The popup shows the command
and runs nothing. A branch with a held checkout, worktree or preview is
shown even with nothing queued. A checkout the next round still tries is
left out.

The tab bar's line for that branch ends `· 1 held: release failed`, and
shows a branch with nothing queued that has one:
`sluice main · 0 submissions · 1 held: release failed`.

## Agents' checks

An agent that runs its checks with `sluice exec` takes one of the runs the
host's bound allows, the same runs the queue's combinations start under.
The popup shows those checks under CHECKING, after the branches, as load
on the host rather than as submissions: a check waits in no queue and
lands nothing.

```
CHECKING
    e2e-run → main ./scripts/e2e.sh 30s
    fix-cards → main 3/8 ▓▓▓░░░░░░░ 1m10s
    plugin-board → main 14 passed · reused when submitted
  host: 3 of 6 runs · 1 queue · 2 agents
```

Each row names the worktree and the branch its checks are held to. A check
under way shows how many of its tasks have ended, or the command it runs.
A check that ended in the last quarter hour says what it is worth. `14
passed · reused when submitted` means the branch's gate takes those passes
once that head is submitted, so the combination it joins replays them
rather than running them again. `not kept: uncommitted changes` means the
worktree held more than its head, so nothing was recorded. The host line
counts the runs against the bound: the queue's combinations and the
agents' checks, those of other repositories on the host included.

When the host has no run free while a submission is queued, and agents' checks
hold some of those runs, the tab bar's line ends with
`· 1 slot to an agent's checks`. That is why a queue tests fewer
combinations at once than its window allows.

When the popup is too short for all of it, each combination keeps its
running and failed tasks and counts the rest, as in
`… 18 passed, 10 waiting`. If it is still too tall, the popup shows
what fits and says how many lines are above and below; the arrow keys
and Page Up and Down scroll it. Opened from a pane outside any git repository, the popup says so
and shows every repository the plugin holds.

| Key | Does |
|---|---|
| `q`, Escape | close the popup |
| `j` | jump to the pane of the submission blamed most recently |
| `a` | switch between this repository and every one the plugin holds |
| ↑, ↓, Page Up, Page Down | scroll a view taller than the popup |
| `1` to `9` | do what a Ctrl-click on the row numbered so does |

The `dashboard` action opens the same view, every repository at once, in a
tab of its own.

In both, a submission and a failed test's `log of` row are links.
Ctrl-click a submission to go to the pane of the agent that made it. Ctrl-click
a failed test to open its log in a split beside the pane. A link acts only on
a worktree or a log the queue names, so a link that some other output
prints does nothing.

## Toasts and the jump

A blame or a conflict always raises a toast naming the submission, the
branch and the tests blamed. A landing raises one only when the submitting
agent's pane is open and is not the one you are looking at. herdr's toasts
take no click: the `jump` action, or `j` in the popup, focuses the pane
whose worktree made the submission most recently blamed or in conflict.

## The sidebar

Each agent's entry gains one row when its worktree has something to say.
herdr's Agents list shows only panes running an agent, so a plain shell
shows no row:

| The worktree | The row |
|---|---|
| has a submission queued or under test | `#95 testing 3m/15m`, `#95 testing 1/2 ▓▓▓▓▓░░░░░ 3m/15m` |
| had its latest submission blamed, or in conflict | `#87 BLAMED · tests:t-cards` |
| is the pane's, and the pane has a branch named (below) | `release · 3 queued · #12 testing 3/8 · 2 checking` |
| is running its checks with `sluice exec` | `checking 3/8`, or `checking 2m` for a command |
| is ahead of its branch | `3 ahead · clean`, `3 ahead · conflicts #95 in builds.md +1` |
| landed in the last 30 minutes | `#94 landed 12m` |

The ahead row asks `sluice submit <branch> --check --json` whether the
worktree would merge with no conflict behind everything queued, and names
the first file in conflict. It asks again only when the worktree's head,
the branch's revision or the submissions queued on it move. Each workspace's row shows how many submissions
its repositories hold, then how many checkouts, worktrees and previews are
held because their release failed: `4 submissions · 1 held: release failed`. A checkout that several
hosts list counts once. The plugin sets every row again within a minute,
and a row expires two minutes after it was last set, so nothing stale stays
if the plugin stops.

## Naming a pane's branch

A row follows the worktree its pane is in. When several agents share one
checkout, each working for a different branch whose submissions come from
other worktrees, the checkout can't say which branch is whose. Whatever
starts the agents names it, with the pane token `sluice_branch`:

```sh
herdr pane report-metadata "$HERDR_PANE_ID" --source my-orchestrator \
  --token sluice_branch=release
```

The row then shows that branch: `release · 3 queued · #12 testing 3/8 · 2
checking`, its submissions in no combination under test, its oldest
combination under test, and the agents' checks held to it; `release ·
nothing queued`, or `release · no records` when sluice has no records of
it. The popup opened over the pane shows that branch first. A submission,
blame or conflict of the worktree's own still comes first, and the pane
gets no ahead forecast, so name a branch only where the worktree doesn't
say it.

Reported without `--ttl-ms`, the token stays until it is replaced,
cleared with `--clear-token sluice_branch`, or the pane closes, but not
across a restart of herdr's server: report it again when the agent is
restarted or resumed. The plugin reads a server's panes, and the token
with them, when the server opens a pane or a worktree. It reads them again
15 seconds or so later, and once a minute while the server shows a row of
the plugin's. A token reported as its pane opens shows within 15 seconds.
One changed later, with no event, shows within a minute, since a named
pane always shows a row. A Claude Code session reads the same from
`SLUICE_BRANCH`, as
[the Claude Code guide](/sluice/guides/claude-code/#naming-a-sessions-branch) says.

## Other sources

By default the plugin runs `sluice overview --watch --json` once in each
repository a pane of any herdr server is in, and stops it once no pane is
there. Any other command that prints the same documents,
one per line, can be added in `sources.toml` in the plugin's config
directory (`herdr plugin config-dir agentplot.sluice`); the
[plugin's README](https://github.com/boomdrivedev/sluice/tree/main/plugins/herdr)
gives its form. `ahead_check = false` there shows the ahead count without
asking sluice, for a branch whose remote is slow to fetch from.
