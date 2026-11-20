---
title: The Claude Code plugin
description: Install sluice's Claude Code plugin, have Claude install sluice and set up a repository, and let your agents submit, read results and run tests through it.
sidebar:
  order: -2
---

The plugin teaches Claude Code to use sluice: to install it, to set a
repository up for it, and to land work through it instead of merging. It
lives in sluice's own repository, so it changes with the commands it
describes.

## Install

At the prompt of a Claude Code session in a terminal:

```
/plugin marketplace add boomdrivedev/sluice
/plugin install sluice@sluice
```

The first line adds the repository's marketplace, named `sluice`; the
second installs the plugin from it. Choose the user scope to have it in
every session.

## Set up a repository

In the repository, ask:

```
set up sluice here
```

Claude checks for the `sluice` binary first. If it is missing or older than
the plugin needs, Claude shows how it would install it and asks first:
through nix where nix is present, else the release binary for your platform,
else `cargo install`. A sluice that comes from your own flake or
home-manager is yours to update, and Claude says so rather than touching it.

In a plain clone, the repository's one working copy, Claude next offers
to lay the repository out with
[one working copy per branch](/sluice/guides/worktrees/#one-working-copy-per-branch),
before it reads or writes anything else. It names each process that has
the folder open, which must let it go and start again from `main/`, the
sessions' history it will carry to the new paths, and what stays behind.
It moves nothing while anything but its own session holds the folder.
Once the folder is laid out, it carries the folder's other sessions and
its project memory across, and asks you to type `/cd <folder>/main` to
bring this session along. It keeps the plain clone only on your word,
and the commit that adds `.config/sluice.toml` then says you chose to.
In a clone that already has other worktrees, each agent can work in one
of its own. Claude makes the same offer there and names the same costs,
then goes on with the repository as it is, without waiting for an
answer. It lays the folder out whenever you ask.

Then Claude reads what the repository already uses and proposes a
`.config/sluice.toml` to match:

- moon, or one test command;
- a sandbox for the tests, on macOS;
- ports for the checkouts, and the host's shared caches in its own config;
- worktrunk's `wt submit` in place of `wt merge`;
- a branch on GitHub, sluice's own GitHub App, and pull requests.

It asks which branches to protect and what must pass on each, shows the file,
and walks through `sluice protect`, the host's `[runs]` settings and, for
GitHub, the app and `sluice forge check`. It ends with one change landed
through sluice to prove the setup.

Before it writes the host's config, Claude checks who writes it. A file
that is read-only, or a link into another tool's store, is managed with
the rest of the host's configuration: Claude says so, shows the settings
to add where the configuration is managed, and leaves the file alone
([who writes it](/sluice/reference/configuration/#who-writes-it)).

## What your agents do with it

In a repository with a `.config/sluice.toml`, Claude:

- submits with `sluice submit` or `wt submit` and never merges or pushes to
  a protected branch. It submits with `--no-wait` and runs `sluice wait
  <id> --timeout 9m --json` in the background, again for as long as it
  exits `5`, so you and other agents can reach it while its work lands,
  and reads the outcome from the record's `state`. It waits in the
  foreground only where it can run nothing in the background;
- commits everything first, and checks for a conflict with `--check`;
- on a failure, reads the blamed tests and their logs, reproduces them with
  `sluice exec`, fixes and submits again;
- on a conflict, merges the branch in and submits again, and, told by
  the board that its work would come to conflict, merges the branch in
  while the conflict is small;
- runs its tests while it works with `sluice exec`, and cleans up with
  `sluice release`;
- brings up a landed revision with `sluice preview` when you ask to try it;
- tells each background agent it starts in a worktree, in that agent's
  prompt, to submit its own work through sluice from there and report the
  outcome;
- starts an agent in a worktree to submit work left there by an agent that
  has ended, and never hands you the submit to run.

## Kept off protected branches

Two hooks come with the skills.

At the start of a session in a repository whose policy protects branches,
Claude is told which ones, and that work lands on them only through
`sluice submit`. It's told too that an agent it starts in a worktree
submits its own work from there, and that you're never asked to run
`sluice submit`.

Before Claude runs a shell command, a guard checks it. A `git push` to a
protected branch, a `git merge` or `git update-ref` onto one, or a
`gh pr merge` into one is refused, with the reason naming the
`sluice submit` command to run instead. Every other command, its own
branch's push included, goes through. The guard reads which branches are
protected from the policy committed in the repository. sluice's own git
hook already refuses a local update; the guard says why before git does,
and covers a push to GitHub and a pull request merge, which that hook
never sees. It needs `jq`, and lets everything through without it.

## The queue in your session

A second plugin, `sluice-board`, shows the queue inside the session:

```
/plugin install sluice-board@sluice
```

It needs no herdr: it shows the queue in any Claude Code session, with
full-screen mode on or off.

<a class="screenshot-light" href="/sluice/screenshots/board-light.webp"><img src="/sluice/screenshots/board-light.webp" alt="Claude Code with the /sluice pane docked on the right: one combination under test at 3/5, pressed open to its tasks, three passed and two running; three submissions queued, this session's among them; a landing and a blame; an agent's check at 4/5; the band above the prompt and the status line below it." width="1248" height="754" /></a>
<a class="screenshot-dark" href="/sluice/screenshots/board-dark.webp"><img src="/sluice/screenshots/board-dark.webp" alt="Claude Code with the /sluice pane docked on the right: one combination under test at 3/5, pressed open to its tasks, three passed and two running; three submissions queued, this session's among them; a landing and a blame; an agent's check at 4/5; the band above the prompt and the status line below it." width="1248" height="754" /></a>

While a branch has a queue, the band above the prompt reads like
`sluice main · 3 submissions · testing main+#12+#13 2m10s/15m · #14 waiting · last #11 landed as 9ecc34a`,
and the status line says where your own submissions stand. `/sluice` opens
a pane with each branch's combinations under test, its queue with your
submissions marked and each pull request linked to GitHub, and its recent landings, blames and conflicts, with the
blamed tests' logs. A branch with nothing queued is left out once a
quarter hour has passed since its last event.

With full-screen mode on, the pane docks on the right of the transcript
and the prompt keeps the keyboard; its close mark or `/sluice` again
closes it. With it off, the pane opens above the prompt and takes the
keyboard, and Esc, its `close` button (or `x`) or `/sluice` again closes
it. The reply to `/sluice` says which.

Where nothing has been submitted yet, the pane says so, and that a
submission shows there from when an agent in a worktree runs `sluice
submit <branch>` until it lands. Where sluice can't be read, because it
isn't installed or the board's watch fails, the pane says why, in sluice's
own words where it gave any. It then says to ask Claude to set sluice up in
the repository. While there is no queue to show, each `/sluice` that opens the
pane starts the watch again at once. Once Claude has set sluice up, the
pane and the band show the queue without a new session.

Where sluice isn't set up in the repository, the band and the status line
stay empty, and the board starts no further watch, so nothing of sluice's
keeps running there. `/sluice` reads once more, and the pane says `Reading
sluice…` while it does. Then it says `sluice isn't set up in this
repository.` and `Ask Claude to set sluice up in this repository.` Once
sluice is set up, `/sluice` shows the queue, and the board follows it from
then on.

Press a blame, or a failed test's `log of` row, to open it under its row:
each failed test, its log path (press it to copy the path) and the log's
last lines. Press it again to close it. The pane only shows: it never
moves to another session or another agent's pane.

A test blamed for being flaky passed on one run and failed on another for
the same inputs. Opened, it shows both runs, each with when and on which
host it ran. First comes this run, as in `this run: passed 19:42 on
devbox`, with its log and last lines. Then comes the run it contradicts,
as in `the run it contradicts: failed 19:10 on mac-studio · main 4f1c2aa`,
with the branch and combination it ran for, and its log and last lines,
or `its log was not kept`. A record from an older sluice leaves out the
times and hosts it lacks.

The pane links what it knows the address of, and nothing else. A
landing's commit links to it on GitHub when the branch's remote is there,
and is plain text for a branch with no remote or one elsewhere. Each pull
request on a QUEUED or RECENT row links to it, as in `· PR 41`, at the
branch's remote, or at origin when the policy names none. Under PREVIEWS,
each preview shows its name, the branch and commit it was made from, as in
`main@4f1c2aa`, its state, and each address its stack answers on, linked.
On desktop, a link that is not `https:`, such as a preview at
`http://localhost:4001`, shows as plain text.

When a checkout's release keeps failing, sluice stops trying and holds the
checkout until you release it
([held checkouts](/sluice/reference/commands/#checkouts-held-because-their-release-failed)).
The pane shows each one first under its branch's heading, under RELEASE
FAILED, with the worktrees and previews of that branch held the same way.
Each row is red and gives how many times the release failed and the last
line of what failed, as in `✗ sluice/try/main/48 held · failed 4 times ·
usage: scripts/preview up`. Beneath it, dimmed, comes the command that
runs the release again, as in `sluice release sluice/try/main/48 ·
--accept-failure frees it without`. A checkout another host made says
`held on deneb`, and its command begins `on deneb:`. The rows read as
herdr's popup does, and pressing one runs nothing. A branch with a held
checkout, worktree or preview is shown even with nothing queued. The band
ends that branch's line with `· 1 held: release failed`, and the status
line ends with the same count, or says it alone when nothing is queued.

Press a conflict to open it the same way. It shows the files in conflict,
then what is in the way: each submission it conflicted with and the
worktree it came from, as in `in the way: #217 (feature/runner)`, or `what
main has landed` when it conflicted with no submission. Last comes what to
do next: `next: take main into the worktree, resolve and commit, then
sluice submit main`. The board builds it from the conflict's own line and
runs no command.

With the moon runner, a combination under test also shows how many of
its tasks have ended and a bar of the same, as in
`testing main+#12+#13 3/5 ▓▓▓▓▓▓░░░░ 2m10s/15m`. Press its row in the pane
to list each task below it: `✓` passed, `✗` failed, `…` running and `·`
waiting. An ended task shows how long it took, and a running one how long
it has run so far, as in `… sluice:e2e running 1m12s`. Press the row again
to fold the list. Each round updates it, every few seconds while a submission
waits, and it counts tasks, not the tests inside one. It shows only for
combinations tested on this host.

Your agents' own runs of `sluice exec` are checks, not submissions, and
the board shows them as load on the host, never in a branch's queue. A
check takes one of the host's runs, the same runs combinations are tested
in, so a busy host can test fewer combinations than a branch allows. When
it does, the band ends that branch's line with `· 1 slot to an agent's
checks`, and the status line says it once. The pane lists each check
under CHECKING, one row per checkout and the branch it checks against:
`fix-cards → main 3/8 ▓▓▓░░░░░░░ 1m10s` while it runs, or the command it
runs. A check that ended stays a quarter hour. `14 passed · reused when
submitted` means the gate takes those passes when that commit is
submitted, so its combination doesn't run them again; `not kept:
uncommitted changes` means the checkout held more than its commit and
nothing was kept. Press a check's row to list its tasks as a combination's
list shows them, from the run going or the one that ended, and press it
again to fold them. A check that runs a command has no tasks, and says so:
`runs make e2e · no tasks`. Under the rows, `host: 4 of 6 runs · 3 queue ·
1 agent` says where the host's runs go.

When one of the session's submissions lands, fails or conflicts, a toast
says so, and the board collects with `sluice inbox --new`, which hands over
only what nothing has handed the session yet. An idle session then gets a
turn in sluice's own words: the outcome, which tests were blamed and where
their logs are, and the key to confirm it by, so the agent can fix its work
without being asked. Where sluice already typed its notice into the
session's herdr pane, or a wait in the worktree reported it, `sluice inbox
--new` prints nothing and no turn starts. When the session starts, and after
the board is reloaded, it collects with `sluice inbox`, which hands over
again every outcome the agent has not confirmed, so a session that
restarted before it acted gets it again. The agent confirms each outcome
with `sluice confirm <key>` once it has acted on it, as the session note
tells it to, and from then on nothing hands that outcome over. The board
keeps no list of its own. The `wake` option in `/config` turns the
collecting and the turn off, and the outcome then stays owed until the
session runs `sluice inbox` or a wait reports it.

The board runs with the sluice releases its plugin names, from the first
with `sluice inbox --new` and `sluice confirm` up to the first it does not
know. Where the host's
sluice is older or newer, the board says so once in the session, in a toast
naming both releases and whether sluice or the board is to be upgraded, and
then shows nothing and hands nothing over; `/sluice` answers with the same
words. Upgrade whichever it names and start the session again.

Before it submits, the board forecasts the session's worktree whenever its
commit, its branch or the submissions queued there move, as `sluice submit
--check` does. The work can come to conflict with what the branch has
landed since the worktree was cut, or with a submission ahead. The session
then gets a turn naming the files and telling it to merge the branch in
now, while the conflict is small. A conflict it was told of can also go
away without its commit moving, when a submission ahead fails or is
withdrawn. The session is told that too, and that a submission removed for
the conflict can go again. While a forecast finds a conflict, the pane
shows it under its branch as `⚠ your work would conflict ·
docs/builds.md`, until a later forecast finds none. Press it to open the
files, what is in the way and the next step, as for a conflict in RECENT. The `forecast` option in `/config` turns this
off; so does `SLUICE_FORECAST=0` in the session's environment, for a tool
that forecasts for its agents itself.

It reads everything from `sluice overview --json --watch`, from the
session's start, and never runs a one-shot `sluice overview`, so it needs
sluice 0.12 or later. Where the host's sluice is older, the band says so
in one line, naming the version sluice gives, as in `can't follow sluice
0.11.2: it has no watch; the board needs sluice 0.12 or later`. The pane
shows the same line and `Ask Claude to upgrade sluice.` in place of the
queue, and the status line is empty. When the watch ends, the board starts
it again, waiting a second and then longer each time up to a minute, and
never polls; the band and the pane keep the last queue read meanwhile.
Once sluice is upgraded, the next queue takes the message's place.
The band and the status line stay silent in a
repository with no queue, and where sluice isn't set up the board starts
no further watch until `/sluice`. It is a Claude Code mod, a newer and still-changing interface, which is why it is a
plugin of its own.

### Naming a session's branch

The status line follows the session's worktree. When several agents share
one checkout, each working for a different branch whose submissions come
from other worktrees, the checkout can't say which branch is whose.
Whatever starts the session names it in the session's environment:

```sh
SLUICE_BRANCH=release claude
```

With no submission of its own, the status line then shows that branch:
`sluice: release · 3 queued · #12 testing 3/8 · 2 checking`. That is its
submissions in no combination under test, its oldest combination under
test, and the agents' checks held to it. An idle branch reads `release ·
nothing queued`, and one sluice has no records of `release · no
records`. The band and the
pane show that branch first, and the pane shows it even with nothing
queued. The variable is read when the session starts. In herdr the pane
token `sluice_branch` does the same, as
[the herdr guide](/sluice/guides/herdr/#naming-a-panes-branch) says.

## Worktrees

Ask about worktrees or a bare repository and the plugin gives the layout
sluice suits best: the repository's git directory kept bare, with `main` and
every branch as worktrees beside it. It converts a normal clone or a `.bare`
directory to it step by step, showing the plan first, moving nothing that an
agent or an editor has open, and deleting nothing.

## What it includes

| skill | when Claude uses it |
|---|---|
| `setting-up-sluice` | installing sluice, writing or changing `.config/sluice.toml`, GitHub, pull requests |
| `using-sluice` | submitting, waiting, reading a failure or conflict, `sluice exec`, previews |
| `worktrees` | the bare repository and worktrees layout, conversions, repairs |

and two hooks: the session note and the guard on shell commands.

Each skill points at the pages of this site for the full story:
[configuration](/sluice/reference/configuration/),
[commands](/sluice/reference/commands/) and the guides.

Without the plugin, the [walkthrough](/sluice/guides/walkthrough/) sets a
repository up by hand.
