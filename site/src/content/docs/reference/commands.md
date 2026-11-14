---
title: Commands
description: The sluice command line, and wt submit.
sidebar:
  order: 2
---

| Command | |
|---|---|
| `sluice` | Where this worktree stands: its submission, each branch's queue, its place, and what to run next. `--json` for a status line. |
| `sluice submit <branch>` | Queue this worktree's head and wait for the outcome; to a branch with pull requests, open one for it instead. `--no-wait` returns once queued or opened; `--status` reports where the last submission from this worktree stands; `--check` only reports whether it would conflict, and `--check --head <branch>` whether a branch no worktree holds would; `--after 2,5` lands it after those; `--json` for scripts. `--head <branch>` without `--check` submits that branch, one sluice lands on, in place of this worktree's head, from anywhere in the repository ([below](#submitting-a-branch-by-name)). `--subject <text>`, one line saying what lands, titles the pull request it opens and names its landing commit. `--worktree <path>` given more than once submits each worktree's head as one act, in that order, with one `--subject` per `--worktree` or none ([below](#submitting-several-worktrees-as-one-act)). |
| `sluice wait <id>` | Wait for a submission's outcome, following sluice's host process, which runs the rounds, as a waiting submit does; `sluice wait <branch>` for this worktree's last submission to it. It runs no round of its own. `--timeout 9m` returns after that long with the submission as it stands, and exits `5`; however short the time, the wait first reads what one pass of the host process left; `--json` prints its record. sluice's host process reads the queue of every repository it holds, and every wait and watch follows what it read ([below](#who-runs-the-rounds)). |
| `sluice inbox` | Hand over every outcome still owed to this worktree's submissions, printing each with the key to confirm it by. Each is handed over again by every inbox until it is confirmed. Run once when a session starts again, or by a tool that submits without waiting. `--confirm` confirms each on receipt, `--new` hands over only what nothing has yet, and `--json` is for scripts ([below](#sluice-inbox)). |
| `sluice confirm <key>...` | Confirm an outcome once it has been acted on, by the key its notice and `sluice inbox` give it, such as `12:landed`, so it is handed over no more. `--as operator` confirms another worktree's, such as one removed before it confirmed. `--json` for scripts ([below](#sluice-confirm)). |
| `sluice status <branch>` | The queue: each submission still in it, by its number and its subject, its agent, its worktree and its state, with how many are in it. Each combination under test says how far it has got. `--all` for every submission the branch has had. It reads the queue where the branch's state last saw it. It reads the policy at the branch's revision as this host holds it: this host's own branch, or for a branch on a remote what this host last fetched of it. It reads both as `sluice overview` does, and never fetches the branch. A branch whose state is on its remote is read from there; when the remote can't be reached, it shows the state last fetched and says so. For a branch on a remote, a round sets this host's own branch of that name to each landing. When it couldn't, status says above the queue which revision this host's branch holds, the landing it lacks and why, until a round sets it. `--json` carries the same under `copy`. While this host's rounds can't read the branch or its state from its remote, status says so above the queue. It says what they couldn't read, from which remote, since when and for how many rounds. Below that is what git said, on as many lines as git said it. It says so until a round reads the remote again. `--json` carries the same under `refused`: `remote`, `asked`, `said`, `since` and `rounds`. Above the queue it also lists the writes sluice's protection refused in the last day, by command, with the host, how many times, the first and the last. A command shows until a day passes with none of its writes refused. `--json` carries them under `refused_writes`, as the overview's fields give them. When another host takes the branch's pull requests in, status lists after the queue each one this host follows. It follows those a submit here opened or updated, those this host handed over, and those a wait or status here asked about. Their heading names the host the branch's intake mark names and whether its `sluice run` or a wait holds it, as this host's last fetch brought it, or says that no host holds the mark. Each row names the pull request's number and title. Then it gives the submission that sluice's check on its head names, and where it stands on that host, or what it waits for there: `#41 Add the export · submission 1406 there · queued behind #38`. Whichever process on the host asks, each is read from GitHub at most once in a quarter of `[forge] discovery`. One found merged or closed shows once, as `landed` or `closed without landing`, and then leaves the list. `--json` carries the same under `taken_in_by`, as a wait gives it, and `pull_requests`, each row with `pull_request`, `title`, `from` (the branch it's opened from), `head`, `draft`, `state` (the check's title, or `waiting` where none stands), `queue` (the check's [`sluice:queue` block](/sluice/guides/pull-requests/#what-sluice-posts), or `null`) and `read_at`. A branch [deleted](#sluice-branch) whose state stays still answers: its first line says `<branch> is gone`, with the revision its state last saw it at, and `--json` gives `gone`, `true` there and `false` otherwise. A branch with no state and no such branch exits 1. It lists the checkouts held because their release failed, each with what failed and how many times it was tried. After the queue, an `OWED` section lists what the branch's outcomes still owe: each worktree or pull request owed one, marked `never confirmed` where nothing has been confirmed under its name, and under it each outcome by its key and submission, owed until or expired at its expiry, with each time it was handed over and how. Nothing is listed where nothing is owed. `--json` carries the same under `owed`, `[]` where nothing is owed: each `reader`, `known`, and `deliveries`, each with `key`, `id`, `state` (`owed` or `expired`), `expires_at` and `handed`. `sluice status <id>` for one. Under `--json`, each submission is its record, and a landed, failed or conflicting one carries `notice`, the one line [`sluice inbox`](#sluice-inbox) prints for it; `sluice status <id> --json` adds its `delivery`. `sluice status` alone lists every worktree sluice holds ports or runs for. `--json` for tools. |
| `sluice overview` | Every branch's queue at a glance: the combinations under test, the submissions still queued, the latest landings, blames and conflicts, and the worktrees and previews held. `--json` for tools: it also gives each branch's `revision`, as the read found it, and `after_landed`, the landed submissions a queued one's `--after` names, and each combination's `on`, the branch's revision it was formed on. `--json --watch` prints it again each time it changes. A read of the queue it couldn't make is printed as one line, the error document with what it was refused. The watch reads again a second later. It ends only when it's stopped, when its repository is gone, or where the repository has no sluice: no state, and no branch here or a remote's as last fetched holding `.config/sluice.toml`. Its last line is then the overview with `"set_up": false`, and it exits `0`; nothing is fetched to tell. One process of the repository on the host reads the queue, `sluice run` where it runs and otherwise the first watch or wait, and the others follow what it read. A watch of a newer sluice takes over the reading from an older watch or wait. `sluice overview` alone always reads it itself. |
| `sluice step <branch>` | One round of a branch's work, then exit: for cron, CI or a hook. sluice's host process runs it between its passes, as it runs every round, and the step prints what it did ([below](#who-runs-the-rounds)). Says `led` when another host keeps the branch, and `held` when the revision it reads lacks the branch's last landing ([below](#sluice-branch)). `--json` for scripts. |
| `sluice amend <id>` | Reorder a queued submission (`--after`, `--priority`) until it is under test. `--branch <branch>` names the branch its number is on, where more than one branch holds it ([below](#sluice-wait)). `--json` for scripts. |
| `sluice withdraw <id>` | Take a submission out of the queue. One already withdrawn is said so, and exits `0`; one landed is refused. `--branch <branch>` names the branch its number is on, where more than one branch holds it ([below](#sluice-wait)). `--json` for scripts. |
| `sluice protect` | Install the branch-protection hook; `--remove` takes it out. Warns of a worktree that has checked out a protected branch. `--json` for scripts. |
| `sluice exec <branch>` | Run the tests this worktree's head affects against the branch, in the worktree, under the host's bound, with the worktree's ports and the branch's limits. `-- <command>` runs a command instead. `--json` for scripts. |
| `sluice equip <branch>` | Make this worktree ready for the branch's runner, as sluice makes every checkout ready before it runs anything there: the toolchain, then under moon the tool versions `.prototools` lists installed, moon's version checked and `moon setup`. It holds nothing. `--at <revision>` reads the policy at that revision; `--json` for scripts ([below](#sluice-equip)). |
| `sluice release` | Release this worktree: run the policy's release command, end what its runs left running, and free its ports. The worktree stays. In a worktree sluice holds nothing for, it runs the default branch's release command all the same ([below](#worktrees-sluice-holds)). `--accept-failure` frees its ports and ends its runs without running the release command. `--json` for scripts. |
| `sluice preview <branch> <revision>` | Bring up a revision the branch has landed in a checkout of its own, at addresses a person opens, until it is released. `--wait` returns once its command has ended; `--json` for scripts. |
| `sluice previews` | Every preview on the host, with its addresses and state. `--json` for scripts. |
| `sluice release <name>` | Release a preview, by its name or its revision: its release command runs, its checkout goes, its ports and names are free. Given a checkout held because its release failed, `sluice/try/<branch>/<n>`, run its release once more. `--accept-failure` frees either without running the release command. `--json` for scripts. |
| `sluice branch create <branch> --from <branch>` | Make a branch sluice lands on, such as a feature branch, at another such branch's current revision: on its remote when its policy names one. `--json` for scripts. |
| `sluice branch delete <branch>` | Delete it once its work has landed into `main`, or the branch `--into` names, and nothing is queued on it. `--json` for scripts. |
| `sluice branch accept <branch> <revision>` | Accept the revision a held branch is read at, which lacks its last landing, as the one it stands on: the next round forms the queue on it. The operator's to do, never an agent's. `--json` for scripts. |
| `sluice forge check [<branch>]` | Whether sluice's own identity on GitHub is set up so only sluice may move the branch there. `--json` for scripts. |

## Forecasting a conflict

`sluice submit <branch> --check` forms the combination the worktree's
head would land in were it submitted now, behind every submission
queued on the branch, and says whether it would conflict. It queues
nothing and changes nothing. A feature branch that no worktree holds
is forecast with `--head`; the submissions made from that branch are
left out, as submitting it again would withdraw them:

```console
$ sluice submit main --check --head feature/runner
would conflict: docs/architecture/builds.md with #217 (wldn-design, design/runner-credential)
```

It exits `0` with no conflict, `2` with one or on a refusal. `--json`
prints `conflict`, null or `{paths, with}`, and what the forecast was
formed from: `head`, the revision forecast; `onto`, the branch's
revision; and `ahead`, the submissions formed ahead of it, in order. A
tool forecasts again only when one of them has moved. A forecast takes
the combinations the queue has already formed on the branch's revision,
so it costs one merge behind them.

Where a watch or a wait on the host is reading the queue and keeping up,
a forecast asks the remote nothing. It takes the branch, the submissions
queued on it and a `--head` branch from what that watch or wait last
read. Otherwise it fetches the branch and its queue first, as sluice's
own rounds do, and the work queued from other hosts where it's missing.

```json
{"conflict":{"paths":["docs/architecture/builds.md"],"with":[{"id":217,"from":{"agent":"wldn-design","worktree":"design/runner-credential"}}]},"head":"4f0c2a1…","onto":"9b7e3d0…","ahead":[217]}
```

## Submitting a branch by name

`sluice submit <branch> --head <branch>` without `--check` submits a
branch sluice lands on, such as a feature branch, to another, in place
of the worktree's head. It runs anywhere in the repository: a worktree
on any branch, the main checkout, or a bare repository's git directory.
It reads nothing of the folder it runs in, so what a worktree holds
beyond its commits refuses nothing.

```console
$ sluice submit main --head feature/runner
submission 52 to main from feature/runner · 1 queued ahead (#51)
testing main + #51 + #52 · 12s of 45m
testing main + #51 + #52 · 4 tasks to run, 9 replayed
passed · landed as b7d2e94
```

It submits the branch as its policy holds it: the remote's revision
where the policy names a remote, and this host's own branch otherwise.
It pushes nothing, moves no branch on this host, and says nothing of
this host's copy of a branch on a remote. `--check --head` with the same
branch forecasts that same revision, and leaves out the submissions the
submit would withdraw, so the forecast says what the submit would meet.

It takes `--after`, `--priority`, `--no-wait`, `--json` and one
`--subject`, as any submit does. `--no-wait --json` answers
`{"id","branch","ahead"}`. With no subject, the landing commit names the
branch. `--head` with `--worktree`, with `--status` or with more than
one `--subject` exits `64` before anything is read. It refuses, exits
`2` and writes nothing for a branch that doesn't exist, `no branch
feature/gone to submit`, and for the branch it would land on, `main is
not submitted to itself`. It refuses a branch no `[branches]` table
names at its revision as well:

```console
$ sluice submit main --head cards
refused: cards is no branch sluice lands on: a branch of a worktree's own is submitted from that worktree
```

The submission is the branch's own work. Its record names the branch as
both `from.worktree` and `from.head`, with no `path` and no `session`
([the state](/sluice/reference/state/#a-submission)). A submission by
name and one from a worktree that has checked out the branch are one
work. Each withdraws the other's earlier submission to the same branch,
and its `attempt` counts both.

Its outcome goes to no session. No round types a notice for it or logs
it `pending`, no `sluice inbox` collects it, in any worktree, it is owed
to no one, and `sluice confirm` finds nothing under its key. `sluice
wait <id>` reports it and records nothing. `sluice status main --all --json` gives it with the words an
inbox would: the submission whose `from.head` is the branch, with its
`notice`, its `failed` or `conflict`, and `tested.on`. A waiting submit
by name runs sluice's rounds as any waiting submit does, and exits by
the outcome. The way on of one that failed is its caller's, and
`sluice status <id>` names it too:

```console
$ sluice wait 53
testing main + #53 · 4 tasks to run, 9 replayed
failed: cli:test
  on main + #53 (e20a6f9)
  log of cli:test: .git/sluice/.logs/main/e2/cli:test.log
once a fix lands on feature/runner, sluice submit main --head feature/runner submits it again
```

For a conflict, the fix that lands on the branch first merges the target
at the revision the submission was tested on, as `once a fix that merges
main at c41e8f2 lands on feature/runner` says.

To a branch that takes pull requests, the submit opens or updates the
pull request from the branch, ready for review, and titles it by
`--subject` or the branch's name. It pushes nothing. sluice takes the
pull request in as any other and posts its outcome on it.

## Refused at submit

`sluice submit` refuses a submission before queuing anything, and exits
`2`, when the worktree holds changes beyond its commits, is on no
branch, or names with `--after` a submission the branch doesn't hold.
It also refuses one when the branch requires a task that the task
runner, in the worktree, keeps out of CI, such as a moon task marked
`runInCI: false`. That task never runs, so the branch can't pass while
it's required.

```console
$ sluice submit main
refused: main requires root:integration (.config/sluice.toml line 59), which the task runner in this worktree keeps out of CI, so it is never run and main cannot pass while it is required
the agent working in integration lets it run in CI there, or drops it from main's policy in the same submission that lets it run, and submits again
```

A task required through a tag names the tag: `main requires app:serve
through #check-fast`. The branch's required tasks are read from the
branch, and whether a task runs in CI from the worktree, so the
submission that lets the task run is taken. To a branch with pull
requests, nothing is pushed and no pull request is opened. When the task
runner can't be asked, its toolchain failing for one, the submission is
taken with a warning saying why, and `--json --no-wait` carries it as
`warnings`.

## Submitting several worktrees as one act

`--worktree` given more than once submits each worktree's head to the
branch as one act, in the order given:

```console
$ sluice submit main --worktree ../feature-a --worktree ../feature-b --worktree ../feature-c
submission 41 to main from ../feature-a · nothing queued ahead
submission 42 to main from ../feature-b · 1 queued ahead (#41)
submission 43 to main from ../feature-c · 2 queued ahead (#41, #42)
```

sluice queues all of them or none. When a worktree holds changes beyond
its commits, is named twice, or keeps out of CI a task the branch
requires, or the branch refuses the act, the answer names each refused
worktree and why, queues nothing, and exits `2`.
sluice writes the submissions to the branch's state in one write. No
round, waiting submit or `sluice wait` ever reads some of them queued and
not the rest, so no combination holds one of them before every one is
queued. Each is queued after the one before it. `--after` and
`--priority` apply to every one alike, and a later submission of higher
priority may still come between them, as it may between any two.

The act takes one `--subject` per `--worktree`, in their order, or
none. The first subject is the first worktree's, and each names its
own submission's landing commit, or titles its own pull request:

```console
$ sluice submit main --worktree ../feature-a --worktree ../feature-b --subject "Runner credentials" --subject "Runner cache"
```

One subject for several worktrees, or any count but one each or none,
exits `64` before anything is pushed or queued.

Once queued, each is a submission of its own. It is blamed alone, set
aside for a conflict alone, withdrawn alone and handed its own outcome, and
`sluice wait <id>` waits on it alone. One that fails or conflicts holds
back none of the others.

A waiting act follows sluice's host process, which runs the rounds,
until every one is decided. It then
reports each one's outcome in the order given, each under `submission
<id> from <worktree>:`, a conflict with its files and the submission it
is contested with:

```console
$ sluice submit main --worktree ../feature-a --worktree ../feature-b
submission 44 to main from ../feature-a · nothing queued ahead
submission 45 to main from ../feature-b · 1 queued ahead (#44)
#44 queued
testing main + #44 + #45 · 12s of 45m
submission 44 from ../feature-a:
testing main + #44 · 12 tasks to run, 0 replayed
passed · landed as b7d2e94
submission 45 from ../feature-b:
conflict: docs/architecture/builds.md
  conflicts with #44 (ops, feature/runner)
the agent working in feature/docs merges or rebases onto main there and submits again
```

It exits `0` when every one landed, and otherwise with the exit of the
first, in the order given, that did not land: `2` above. Stopped with
Ctrl-C, it exits `130` and the submissions stay queued. `--no-wait`
exits `0` once all are queued.

Under `--json` it prints one document however it ends,
`{"branch","submissions"}`, one item per worktree in the order given,
each with its `worktree`. With `--no-wait` an item holds `id` and
`ahead`, and `warnings` when it was taken unchecked; waiting, it is the submission's record, as `sluice status <id>
--json` prints it but for its `notice`. A refusal's error document lists the refused
worktrees under `worktrees`. `--check` and `--status` take one worktree,
and exit `64` given more.

To a branch that takes pull requests, the act pushes each branch and
opens or updates its pull request, then takes every pull request or none.
When each meets its reviews and checks, sluice marks every one ready for
review and queues all of them at once, each said with its submission,
`submission 61 to main from #51 · nothing queued ahead`, and each item
under `--json` holding its `pull_request` too. When any falls short, it
queues none, leaves every one a draft, names each short with what it
lacks, and exits `2`; run it again once they're reviewed. Once queued, new
commits that no submit pushed make that one a draft until its worktree
submits again, and the rest land. The [pull requests
guide](/sluice/guides/pull-requests/#several-pull-requests-as-one-act)
says more.

## `sluice exec`

An agent runs its tests while it works with `sluice exec`, in its own
worktree. The run counts against the host's bound for as long as it runs,
waiting while the bound is full, and never uses the part
`[runs] reserved` keeps for submissions. It gets the worktree's ports,
the same on every run, runs under the branch's executor, memory,
network and time limit, and ends with its process group. The tests are
the ones the head affects against the branch, read from the branch's own
policy, never the worktree's.

```console
$ sluice exec feature/concurrent-units
running the checks 8d41b6c affects against feature/concurrent-units (c41e8f2)
ports: SLUICE_PORT_DB=20001 SLUICE_PORT_WEB=20000
…what the tests print…
passed: 2 tasks
recorded 2 results under their keys for 8d41b6c
```

When the worktree holds exactly its head, with nothing modified, staged
or untracked, each test's result is recorded under its key as an open
run. A later submission's combination with the same key reads the
result on a branch that trusts open runs. Its runner replays the test
when the host's [`[[caches]]`](/sluice/reference/configuration/#caches)
share the runner's cache among the host's worktrees, the same condition
as between combinations. A worktree holding anything more runs the same
tests, records nothing, and says `not recorded: worktree is not clean`.

Before it runs, exec forecasts the head against the branch, as `sluice
submit --check` does, and says when it would conflict: `would conflict:
<files> with <what>`. The run goes on either way, its exit is the
run's, and nothing of the forecast is recorded. `--json` carries it as
`forecast`, in `--check --json`'s shape, or null for a detached
worktree.

A test that could not run is listed as `could not run: <test>: <why>`
and has no result, so nothing is recorded under its key. A test could
not run when its program could not be started, when it exits with a code
`[tests] not_run` names, or when the executor reports refusing the run
something it needs.

When a task a required test depends on runs and fails, and the task
runner skips the test for it, that task fails the test. exec lists the
task under
`failed:`, with the tests that failed on their own, and beneath it says
which test the task failed. It records the task's fail under its
key, as it records a test's, and exits `1`. It never names the skipped
test as not run: it never ran, and the task is the one to fix.

```console
$ sluice exec main
running the checks 8d41b6c affects against main (c41e8f2)
…what the tests print…
failed: api:integration
  root:integration failed by api:integration, a task it depends on
recorded 1 result under its key for 8d41b6c
```

Under `--json`, `dependencies` gives each task that failed a test as
`tasks` gives a test, `{task, hash, outcome, recorded}`, and
`failed_by` gives each test a task failed, `{task, by}`, `by` naming
the tasks. Neither is in `missing`, which lists only the required tests the
run left without a result.

`sluice exec <branch> -- <command>` runs any command the same way, with
`SLUICE_CHECKOUT`, `SLUICE_TMP` and the ports in its environment, and
exits with the command's exit code. It records nothing, and is not held
to `not_run`. Before the command, sluice equips the worktree as it
equips a combination's checkout, by the runner the branch's policy
names ([`sluice equip`](#sluice-equip)). Under moon, the command runs
once `moon setup` has, though it runs in place of moon's tasks. When a
step fails, the command never starts, and exec exits `3`, naming the
step and what it printed.

Run suites through `sluice exec` so the agents on one host take turns
under its bound rather than all running at once. A quick, narrow call,
such as a type check or one unit test, can run directly.

While it runs, `sluice overview` shows it as a check on the worktree,
never as a submission: how many of its tasks have ended under the moon
runner, or the command it runs, and when it started. Each check takes a
run of the host's bound, so when the bound is full and a submission
waits, the branch's counts line says how many runs the checks hold:
`testing 2 at once (up to 3) · 1 slot to an agent's checks`.

What a run starts and hands to another process, a container above all,
stays up between runs, so the next run finds it. `sluice release` ends
it. `wt submit` releases the worktree before worktrunk removes it; on
plain git, run `sluice release` before `git worktree remove`.

## `sluice equip`

Before sluice runs anything in a working copy, a combination's checkout,
an agent's worktree or a preview's checkout, it equips that working copy
by the branch's policy, in this order:

1. [`[tests] toolchain`](/sluice/reference/configuration/#tests-how-tests-run),
   where the policy names one, whose output is the environment of every
   step after it.
2. Under the moon runner, `proto install --config-mode upwards`, where
   the working copy holds a `.prototools`: the tool versions it lists,
   installed.
3. `moon --version`, which has to say moon 2.6 or later.
4. `moon setup`.

Under the `command` runner, the toolchain is the only step. Each step
runs outside the executor, with the environment a run of the branch's
tests starts from and nothing else of the caller's. The first step that
fails ends the sequence. A combination whose checkout could not be
equipped could not start: its tests are not run, each named with the
step and what it printed, and nobody is blamed.

`sluice equip <branch>` runs those steps, through `moon setup`, in the
worktree it runs in, and nothing else. Run it in a worktree sluice did
not make, before anything runs moon there by hand: a fresh clone, or a
worktree a tool made for an agent. A tool that makes worktrees for
agents runs it in each, so each one is ready as sluice's own checkouts
are. Run from any folder of the worktree, it equips the worktree's top,
where its `.prototools` is.

```console
$ sluice equip main
equipping /home/me/wt/unit-a for main at c41e8f2
toolchain: passed
proto install: passed
moon --version: passed
moon setup: failed
  exited 1: error: no such toolchain
```

Each step gets its own line, saying how it went. A failed step's words
follow it, indented: sluice's reason, `exited 1` here, then what the
step printed. Under the `command` runner with no toolchain, it says
there is nothing to equip.

It reads the policy at the branch's current revision. `--at <revision>`
reads the policy at that revision instead, whether or not its
`[branches]` tables name the branch. The revision need not be one the
branch has landed, so a worktree made from another revision is equipped
as its own policy says.

`--json` prints one document on stdout and nothing on stderr:

```console
$ sluice equip main --at 2eb676c --json
{"branch":"main","revision":"2eb676c8e45d…","steps":[{"step":"toolchain","passed":true,"words":""},{"step":"proto install","passed":true,"words":""},{"step":"moon --version","passed":true,"words":"moon 2.6.1"},{"step":"moon setup","passed":true,"words":""}]}
```

`steps` lists each step as it ran: its name, whether it passed, and its
words, what it printed, its output then its error stream. The
toolchain's words are its error stream alone, since its output is the
environment. `revision` names in full the revision whose policy was
read.

It exits `0` when every step passed, or when there was none, and `1`
when one failed. It refuses, runs nothing and exits `2` for a branch it
can't read, a revision that is no commit here or holds no
`.config/sluice.toml`, and a folder in no worktree. Without `--at`, it
refuses a branch whose policy names no `[branches]` table for it. Under
`--json` a refusal is the error document, with no `steps`.

It holds nothing: no ports, no record of the worktree and no part of
the host's bound, and it never waits for room in it. Nothing of its own
is left once it answers, so `sluice release` there runs the policy's
release command, as in any worktree sluice holds nothing for.

## Worktrees sluice holds

A worktree keeps its ports and what its runs left running until it is
released, however long since its last run: sluice cannot tell an agent
that is thinking from one that is gone. `sluice status` with no argument
lists every worktree it holds in the repository, with its branch, its
last run, how that run ended and its ports, so a person or a tool can
release one with `sluice release --worktree <path>`. `--json` gives the
same as one document, a `worktrees` list with `worktree`, `branch`,
`last_run`, `ports`, `removed` and, once a run there has ended,
`last_result`.

```console
$ sluice status
worktrees held until released:
  /home/me/wt/unit-a  feature/concurrent-units  last run 2026-10-14T09:12:40Z  last result passed on 1a2b3c4, recorded  SLUICE_PORT_DB=20001 SLUICE_PORT_WEB=20000
  /home/me/wt/unit-b  feature/concurrent-units  last run 2026-10-11T16:03:05Z  last result command failed on 5d6e7f8  SLUICE_PORT_DB=20003 SLUICE_PORT_WEB=20002
release one with: sluice release --worktree <worktree>
```

`last_result` says how the last `sluice exec` in the worktree ended:

| field | |
|---|---|
| `at` | when it ended; `last_run` is when it started. A run under way, or one whose `sluice exec` was killed, has no `last_result` |
| `head` | the worktree's commit it ran on, in full |
| `kind` | `checks`, or `command` for `sluice exec <branch> -- <command>` |
| `outcome` | `pass`, `fail`, `timed_out`, `lost` (the run stopped without saying how it ended) or `stopped` (ended by its caller) |
| `recorded` | its results are kept for their keys, so a submission of `head` reuses them. Never true for a command, or for a run with files changed and not committed. Never true when a commit or an edit was made while it ran: it names the commit it started on. Never true for a run that timed out, was lost or was stopped |
| `tasks` | for a run of the checks that reported, each task as it ended: `target` and `state`, `passed`, `failed` or `replayed` |
| `reused` | for a run of the checks that reported: true when `recorded` is, every check passed, and the branch's gate trusts the executor it ran under. A submission of `head` then replays its results rather than running them again |

The next run replaces it, and `sluice release` removes it with the rest.
A worktree's head is ready to submit when `kind` is `checks`, `outcome`
is `pass`, `recorded` is true and `head` is the worktree's head now. Its
files may have changed since; that is for the reader to check, since
sluice reads nothing from the worktree to list it.

A worktree sluice holds nothing for, one where no `sluice exec` ran or
one already released, is released all the same. `sluice release` there
runs the release command the policy of the repository's default branch
names, in the worktree and with no ports, and sluice keeps nothing for
the worktree after. It exits `0` when the command passed and `1` with
what the command printed when it failed, and says `nothing to release`
only when the policy names no release command. So a driver that removes
the worktree after `sluice release` ends what the worktree's own
commands started, whether or not they ran under `sluice exec`.

A worktree that has checked out a branch sluice lands on is named
too, whether sluice holds anything for it or not. sluice moves that
branch under it and leaves its files as they were, so `git status`
there shows each landing undone. `sluice status`, `sluice overview` and
`sluice protect` warn of it, and in `--json` its entry carries
`holds_queued_branch`, the branch. Nothing else changes: the landing
goes through, and the worktree is left as it is. Look at the branch from
a detached worktree instead; see
[worktrees beside sluice](/sluice/guides/worktrees/).

```console
$ sluice status
no worktree holds ports or runs here
warning: /home/me/repo/main has main checked out: sluice moves main under it and leaves its files behind; look at it from a worktree switched with git switch --detach
```

A worktree removed without a release is no agent's. The next `sluice
exec` on the host, or the next round, releases it from what sluice
holds for it. The release command its last run stored runs once, in its
scratch directory, with its ports and the environment its runs had.
Neither the toolchain nor the policy is read again, since the directory
they would run in is gone ([ADR 0055](/sluice/decisions/0055/)).
Whatever that command does, the worktree's runs end, its ports are free,
and it is never tried again. A release that tears a stack down by name
or port still does. One that reads files from the worktree finds none
and fails, and what it would have removed is left for a person to find.
That command says so first. The branch's state logs it `gone`, with how
its release went, once:

```console
$ sluice exec feature/concurrent-units
freed /home/me/wt/unit-c: it no longer exists; its release ran once from its scratch directory; its runs ended, its ports free
running the checks 8d41b6c affects against feature/concurrent-units (c41e8f2)
…
```

When its release fails, sluice says `its release failed once from its
scratch directory:` and what it printed; when its runs stored none, `it
had no release to run`.

Until then, `sluice status` lists it as removed.

A worktree's `sluice release` whose release command fails exits `1`
with what the command printed. The worktree keeps its ports, and
`sluice status` lists it with what failed until a `sluice release`
succeeds. No round releases it: `sluice release --accept-failure` frees
its ports and ends its runs without the release command, and leaves the
worktree. In `--json` its entry carries `release_failed`: `failed`,
`tries`, `at` and `stopped`.

## Checkouts held because their release failed

When a combination's checkout can't be released, its release command
failing, the checkout stays, held. The branch's state names it with what
failed and how many times it was tried. The next rounds try it again, one
try a round, as many times as the branch's
[`release_retries`](/sluice/reference/configuration/#release_retries)
allows, then stop and say it is held. A held checkout never holds its
branch: the same rounds land what passed, deliver outcomes and test the
next combinations in other checkouts. `sluice step` says each try that
failed in the round it asks for, and a waiting submit or `sluice wait`
each try in its branch's rounds, and the hold once:

```console
could not release sluice/try/main/48 (try 2 of 4): the release command of /home/me/repo/.git/sluice/.places/sluice+2Ftry+2Fmain+2F48 ended exit status: 2: usage: scripts/preview up
sluice/try/main/48 is held: its release failed 4 times: … `sluice release sluice/try/main/48` runs it again; `sluice release sluice/try/main/48 --accept-failure` frees it without
```

`sluice status <branch>` lists the branch's held checkouts, and
`--json` as `held_checkouts`, each with `place`, `failed`, `tries`, `at`,
`stopped` (tried no more) and, on a branch whose state is on its
remote, the `host` that made it:

```console
$ sluice status main
main at b7d2e94 · 0 in the queue · testing 0 at once (up to 2)
held checkouts, whose release failed:
  sluice/try/main/48  4 tries, held, last at 2026-10-23T09:14:02Z
    | the release command of /home/me/repo/.git/sluice/.places/sluice+2Ftry+2Fmain+2F48 ended exit status: 2: usage: scripts/preview up
release one with: sluice release <checkout>, or without its release command: sluice release <checkout> --accept-failure
```

Once you have fixed what the release needed, `sluice release
sluice/try/main/48` runs it once more, whatever its tries; it exits `1`
and stays held if it fails again. `sluice release sluice/try/main/48
--accept-failure` gives up on it: its runs end, the checkout sluice made
is removed, whether or not git still lists it as a worktree, and its
ports are free. The release command is not run, so whatever it would
have removed is left. A checkout is released on the
host that made it. A held checkout doesn't keep its host from claiming:
the host claims runs of its branch as any other does, in other
checkouts. The held one keeps its ports and disk until it is released.

A checkout is gone when someone deleted its directory, or when git no
longer lists its directory as a worktree, as git leaves one whose removal
it stopped part way. Held or not, a gone checkout has the release its run
stored run once from its scratch directory by the next round. sluice
removes what is left of its directory, the checkout is freed whatever the
release did, and it is logged `gone`.

A held checkout also shows wherever you watch the queue, on every host.
[`sluice overview`](#sluice-overview) names it under its branch.
[herdr](/sluice/guides/herdr/) and the
[Claude Code board](/sluice/guides/claude-code/) show it under `RELEASE
FAILED`. Each says what failed and gives the commands that release it.
They show a worktree or a preview held because its release failed the
same way. A checkout the next round still tries shows only in `sluice status`.

## `sluice overview`

`sluice overview` shows every branch of the repository that has a
queue, read from its state alone: it starts nothing, releases nothing
and writes nothing, and it reads with no sluice running.

```console
$ sluice overview
feature/concurrent-units at c41e8f2 · 2 in the queue · testing 1 at once (up to 4)
  sluice/try/feature/concurrent-units/9 is held: its release failed 4 times: usage: scripts/preview up
    sluice release sluice/try/feature/concurrent-units/9 runs it again; sluice release sluice/try/feature/concurrent-units/9 --accept-failure frees it without
  testing feature/concurrent-units + #13 Batch review cards + #14 forecast · 3/5 tasks · running tests:t-cards 2m10s · 6m02s of 15m
  2026-10-16T09:15:02Z  landed #12 Fix the forecast's dates as c41e8f2
  2026-10-16T09:11:40Z  blamed #11 sidebar on 9a03c1e
checking /home/me/wt/unit-a → feature/concurrent-units 3/8 since 2026-10-16T09:17:02Z
host: 2 of 4 runs · 1 queue · 1 agent
2 worktrees held · 0 previews
```

The `host:` line says the runs the host holds, and, while any wait for
room, how many: `waiting: 1 combination, 2 agents`, ending `an agent's
next` where the next contested run is an agent's check's. `sluice
status` with no branch says the same line.

The overview names each submission by its number and the subject its
submit gave it, or, where it gave none, the branch it was submitted
from. A line
names a submission so once; the same number again on that line stands
alone. `sluice status` and `sluice` with no command name them the same
way. A subject is shown on one line: each run of line breaks or other
control characters in it is shown as one space.

Under a branch's line come the checkouts, worktrees and previews of
that branch [held because their release failed](#checkouts-held-because-their-release-failed).
Each says how many times its release failed and the last line of what
failed, then gives the commands that run its release again or free it
without.
A checkout another host made is held on that host, and its commands are
given to run there. One whose branch has no state comes after the
branches, under `<branch> · no records`.

After them come the writes sluice's protection refused on the branch in
the last day, the three refused last, each with how many times, the
host, the last time and the command. Where more were refused, a last
line says how many and to run `sluice status <branch>` for them all.

When another host keeps the branch on its remote, the branch's lines
end with the pull requests this host follows, as `sluice status
<branch>` lists them: a heading naming the host that keeps it, then each
request beneath it, such as `#41 Add the export · submission 1406 there · queued
behind #38`. `sluice overview` alone reads them as status does, each
from GitHub at most once in a quarter of `[forge] discovery` whichever
process on the host asks. A watch, and sluice's host process, which
reads for every watch and wait, show them as this host last kept them,
and never ask GitHub for them.

A branch [deleted](#sluice-branch) keeps its state, and the overview
shows it as `<branch> is gone, last at <revision>`, the revision its
state last saw, with its latest landings under it. One with nothing
queued, under test or recent, and nothing of it held, is named on one
line after the branches instead, `gone, nothing queued or recent:` and
each such branch.

A branch whose policy, revision or state couldn't be read is shown as
`<branch> could not be read:` and what refused it, in its own words,
such as a policy sluice no longer accepts. Every other branch is shown
as usual. The overview fails whole only when it can't list the
branches' state at all.

`--json` prints one document, `sluice/overview@1`, for a pane, a status
line or a script. Fields are only ever added to it.

| field | |
|---|---|
| `schema`, `host`, `repository`, `at` | the schema, the host's name, the repository's git directory, and when it was read |
| `branches[]` | each branch with a queue: `branch`, `at` (its revision as its state last saw it), `version` (moves on every write), `window`, `parallel`, `remote` and `depth` (submissions still queued, waiting, testing, passed or landing). `remote` is the name of the git remote the branch's policy lands it on, as the policy writes it, never its URL; a branch kept in this host's repository has none. The branches come in the order the default branch's policy names them |
| `branches[].candidates[]` | each combination: `id`, `submissions`, `revision`, `state`, `started`, `limit` (seconds), `targets` (the tests the branch requires), `executor`, `ran`, `replayed`, and `failed`, each failed test with where its output is kept |
| `branches[].candidates[].failed[].flaky` | a test read as failed because its inputs both passed and failed. `here` is what this run gave, `at` when its result was recorded and `host` the host that ran it. `other` is the result it contradicts: its `outcome`, `at`, `host`, `branch`, `combination` and `log`. A submission's `failed` in `sluice status <id> --json` gives the same. Records written before `at` and `host` leave them out |
| `branches[].candidates[].tasks[]` | while a combination is under test with the moon runner, each of its tasks: `target`, `state` (`waiting`, `running`, `passed`, `failed` or `replayed`), and, once known, `started`, `ended` and `exit`; left out when nothing has been said of them |
| `branches[].submissions[]` | each submission still queued, as `sluice status <id> --json` prints it |
| `branches[].recent[]` | the branch's last ten `landed`, `blamed` and `conflict` lines of its log, oldest first, exactly as written |
| `branches[].recent_from` | each submission the recent lines name, a landing's `ids`, a blame's `id` and a conflict's `id` and `with`, mapped to the worktree it came from, as its `from.worktree` says, a pull request's as `pull/<n>`: `{"12": "fix-a", "13": "pull/41"}`. It's `{}` when the branch has no recent lines |
| `branches[].recent_subjects` | the subject each submission `recent_from` names was given at its submit, as written, whether it is still queued or has moved to the finished records: `{"12": "Batch review cards"}`. A submission whose submit gave none has no key, and a reader names it by `recent_from`. It's `{}` when no recent submission has a subject |
| `branches[].gone` | `true` where the branch is gone from this host: its state stays, and the host holds no revision of it, as after [`sluice branch delete`](#sluice-branch). `sluice status <branch>` answers it all the same. Left out otherwise |
| `branches[].held` | while the branch is held ([`sluice branch`](#sluice-branch)): `read`, the revision it is read at, `lacks`, the landing that revision lacks, and `since`; left out otherwise |
| `branches[].held_checkouts[]` | the branch's checkouts whose release failed, as `sluice status <branch> --json` lists them: `place`, `failed`, `tries`, `at`, `stopped` (tried no more: held) and, on a branch whose state is on its remote, the `host` that made it. Read from the branch's state, so every host's overview lists them. It's `[]` when there are none |
| `branches[].refused_writes[]` | the writes sluice's protection refused on the branch in the last day, one for each command and host, the last refused first, as `sluice status <branch> --json` lists them. Each has `host`, `ref`, `command`, `times`, `first` and `last`, and the last refused write's `from` and `to` (`null` for a deletion). `command` is the command as git ran it, each full revision in it as `<revision>`, and `times` how many of its writes were refused. Read from the branch's state, so every host's overview lists them. It's `[]` when there are none. A tool that watches the host reads them here, or from `sluice status <branch> --json`, and never from `log.jsonl` or `branch.json`: the log says each command once, and the state's form is sluice's own |
| `branches[].taken_in_by` | when another host keeps the branch on its remote and this host follows any of its pull requests: `{"host"}`, the host the branch's state names as keeping it, which takes its pull requests in, as this host last fetched it, or `null` where no host keeps it yet, as `sluice status <branch> --json` gives it. Left out otherwise |
| `branches[].pull_requests[]` | beside `taken_in_by`, each pull request this host follows into the branch whose submission its own queue doesn't hold, as `sluice status <branch> --json` lists them: `pull_request`, `title`, `from` (the branch it's opened from), `head`, `draft`, `state` (the check's title, `waiting` where none stands, or `landed` or `closed without landing` once found so), `queue` (the check's [`sluice:queue` block](/sluice/guides/pull-requests/#what-sluice-posts), or `null`) and `read_at`, when a process on the host last read it from GitHub. A watch's documents carry them as this host last kept them, and `read_at` says how old each is. Left out with `taken_in_by` |
| `branches[].unread` | when the branch's policy, revision or state couldn't be read: what refused it, in its own words. Nothing else of the branch is read, so its other fields are empty; left out otherwise |
| `sessions[]` | the worktrees held, each with how its last run ended (`last_result`), and those that have checked out a branch sluice lands on (`holds_queued_branch`), as `sluice status --json` lists them |
| `sessions[].run` | while a `sluice exec` runs in the worktree: `started`, `command` for a run of a command, and `tasks`, as a combination's, while the moon runner says them |
| `previews[]` | the previews on the host, as `sluice previews --json` lists them |
| `set_up` | `false` on a watch's last line, where the repository has no sluice: no branch has state, and no branch here or a remote's as last fetched holds `.config/sluice.toml`. Left out otherwise |
| `room` | the host's runs: `bound`, `queue` (the combinations under test, and those about to start), `sessions` (every `sluice exec` and preview command running on the host) and `reserve` (the part of the bound `[runs] reserved` keeps from `sluice exec`). `waiting` gives the runs waiting for room by kind, as sluice's host process holds them: `queue`, combinations whose round asked for room within thirty seconds, and `sessions`, every `sluice exec` and preview command waiting. `turn` is whose the next run a combination and an agent's check both want is: `queue` or `sessions`. `repositories` gives each repository on the host with a run under test or waiting, by its git directory: `under_test`, its runs holding a part of the bound of any kind, and `waiting`, its runs waiting. A run that frees goes to the repository with a combination waiting and the fewest under test. Where no host process answers, nothing waits, `turn` is `queue` and `repositories` is empty |
| `host_process` | sluice's host process that answers, which runs the rounds and holds the room: its `version`, `pid` and `since`, when it started. `null` where none answers. `host` stays the host's name |

A combination's `tasks` say how far its tests have got and decide
nothing: its result is still read from the finished run. Each round
updates them, about every 3 seconds while a submission waits, from
what moon writes as each task ends, and they go once the combination
is no longer under test. They are kept on the host that runs the
combination, so another host shows none. The `command` runner has one
test, the run itself, and gives none.

`sluice status` and a waiting submit say a combination's progress as
the text form does. It counts the tasks that have ended and names up to
three running ones, longest first, with how long each has run. Then it
gives the combination's time against its limit. A running task's time
starts when moon printed that it had started, which the run notes as
its output arrives. Where no tasks are kept, the text gives the
combination's time alone.

A check's `run` comes from the host's own state, written by its `sluice
exec` as each task ends, and goes when the run ends; its `last_result`
then says what it is worth. Like a combination's tasks, it decides
nothing.

The branches come in the order of the `[branches]` tables of the
policy on the default branch, the branch the repository's HEAD names.
A pattern's branches stand together where the pattern does, in name
order, and branches no table names come last. Each branch's gate is
still its own policy's.

`--json --watch` prints the document as one line at once, and again
each time anything in it but `at` changes, until stopped. sluice's host
process reads the queue about once a second while a watch follows it. A reader finds what happened since its last line by
comparing `recent`. A branch's revision is the one its state last saw,
never fetched, so a branch on a remote costs it nothing to watch.

The watch ends where the repository has no sluice: no branch has state,
and no branch here, nor any remote's as last fetched, holds
`.config/sluice.toml`. Its last line is the overview with `"set_up":
false`, and it exits `0`. Nothing is fetched to tell, and a policy
written in a worktree but committed on no branch doesn't count. A
repository set up with nothing submitted yet is watched as usual.
Every watch of the repository prints that same last line and ends too.

A submission's `from.path` names the worktree it came from, so a pane
or a session can find its own submissions in the document.

## `sluice` with no command

`sluice` alone shows where this worktree stands, cut from the overview
to the worktree by the path it submits from, or, for a pull request, by
the branch it was opened from: its latest submission
still queued, or else the latest the branches' recent lines name; each
branch's counts; its place; and the commands that follow. Like the
overview it reads the state alone and writes nothing, so a status line
may run it every few seconds.

```console
$ sluice
worktree cards (agent-2) at /home/me/shop.cards
#14 Forecast concurrent units to feature/concurrent-units · testing
feature/concurrent-units: 2 in the queue · testing 1 at once (up to 4)
place: SLUICE_PORT_WEB=20000 · last run 2026-10-16T09:16:44Z
next: sluice status 14
next: sluice withdraw 14 --as agent-2
```

`--json` prints one line, `sluice/home@1`, whose fields are only ever
added to: `worktree` (`path`, `branch`, `agent`), `submission` (`id`,
`branch`, `state`, `failed` with each test's `log`, `landed_as`, and
`subject`, the submit's words as written, or null where it gave none),
`branches[]` (`branch`, `depth`, `testing`, `parallel`, the overview's
counts, and `unread` as the overview gives it), `place` (`ports`, `last_run`) and `next`. Outside a worktree
sluice can submit from, `worktree`, `submission` and `place` are null.
A submission from before `from.path` was recorded names no worktree and
is not shown. A pull request still waiting for review or checks has no
submission yet; `sluice submit <branch> --status` follows it.

## `sluice preview`

A preview is a running copy of a revision a branch has landed, for a
person to try: the repository's own stack, brought up by the policy's
[`[preview]` command](/sluice/reference/configuration/#preview-a-running-copy-of-a-landed-revision)
in a checkout of its own, with a port and a name for each of
`[checkout] ports`. It is held until something asks for its release. The
[previews guide](/sluice/guides/previews/) says what it is for and how a
tool of your own drives it.

```console
$ sluice preview feature/concurrent-units 2eb676c
preview/2eb676c8e45d of feature/concurrent-units at 2eb676c: starting
  api  http://localhost:20001/docs
  web  https://web-2eb676c--feature-concurrent-units--shop.localhost
  ports: SLUICE_PORT_API=20001 SLUICE_PORT_WEB=20000
```

Only a revision in the branch's history is previewed: the head of a
submission still queued, or a combination under test, is refused. The
addresses are printed as soon as the command starts, filled in from the
policy's templates. The command counts against the host's bound while it
runs, waiting while the bound is full and never using the part
`[runs] reserved` keeps for submissions; once it has ended, the preview
counts against nothing. Ctrl-C while it waits leaves the preview to be
started by the next `sluice preview` of that revision.

Before the command, sluice equips the preview's checkout as it equips a
combination's ([`sluice equip`](#sluice-equip)): under moon, it installs
the tool versions `.prototools` lists, checks moon's version and runs
`moon setup` first. When a step fails, the command never starts, and
`sluice preview` exits `3`, naming the step and what it printed.

Asking again for a revision that has a preview gives that preview, its
names, ports and state unchanged, and starts nothing, so a caller that
cannot tell whether it asked can ask. A revision has one preview on the
host, whichever branch it was asked for through.

A preview is `waiting` for room, `starting` while its command runs,
`up` once the command exited 0, or `failed`. A failed preview is
released at once, with what its command printed kept, and stays listed
until it is released. `--wait` returns once the command has ended and
exits 1 for a failure. `--json` prints one document:

```console
$ sluice preview feature/concurrent-units 2eb676c --wait --json
{"name":"preview/2eb676c8e45d","branch":"feature/concurrent-units","revision":"2eb676c8e45d…","state":"up","made":"2026-10-14T09:12:40Z","checkout":"/home/me/shop/.git/sluice/.previews/preview+2F2eb676c8e45d","addresses":{"api":"http://localhost:20001/docs","web":"https://web-2eb676c--feature-concurrent-units--shop.localhost"},"names":{"SLUICE_NAME_API":"api-2eb676c--feature-concurrent-units--shop","SLUICE_NAME_WEB":"web-2eb676c--feature-concurrent-units--shop"},"ports":{"SLUICE_PORT_API":"20001","SLUICE_PORT_WEB":"20000"},"exit":0}
```

A failed preview's document carries `output`, the end of what its
command printed. Nothing of a preview is recorded as a test result, and
no submission waits on it.

`sluice previews` lists the host's previews the same way, as a list
with `--json`. It first releases any preview whose command failed, and
any whose checkout someone removed by hand. For the latter, the release
its command's run stored runs once in its scratch directory, with its
ports and names; the preview is freed whatever that did, and never
tried again. `sluice release <preview>` releases one: its command's run
ends, the release command runs in the checkout, the checkout is removed
and its ports and names are free. No round, step or landing ever
releases a preview that is up. A preview whose release command fails is
tried again as a combination's checkout is, then held; `sluice previews`
shows what failed, and `--json` carries it as `release_failed`.

## `wt submit`

`wt-submit` is installed beside `sluice`, and
[worktrunk](https://github.com/max-sixty/worktrunk) runs any `wt-<name>`
on the `PATH` as `wt <name>`. It is a driver: it calls `sluice submit`
for the worktree it is run in, waits, and on landing removes the
worktree and its branch as `wt merge` did, following worktrunk's
`[merge] remove`. `--no-remove` keeps them. [sluice with
worktrunk](/sluice/guides/worktrunk/) has the move from `wt merge`.

```console
$ wt submit feature/concurrent-units
submission 12 to feature/concurrent-units · nothing queued ahead
testing feature/concurrent-units + #12 · 4s of 15m
testing feature/concurrent-units + #12 · 31/33 tasks · running tests:t-cards 1m10s, tests:e2e 40s · 1m52s of 15m
testing feature/concurrent-units + #12 · 32/33 tasks · running tests:t-cards 1m31s · 2m13s of 15m
testing feature/concurrent-units + #12 · 2 tasks to run, 31 replayed
passed · landed as b7d2e94
```

While it waits, it says its combination's progress again each time the
count of ended tasks or the running tasks change, as `sluice overview`
shows them.

Submitting again from the same worktree withdraws that worktree's earlier
submission.

### To a branch with pull requests

When the branch's policy has a
[`pull_requests` table](/sluice/reference/configuration/#branchespatternpull_requests),
`sluice submit` and `wt submit` don't queue the worktree's head. They
push the worktree's branch to the branch's remote and open a pull
request from it, ready for review, or update the one already open from
that branch. `--subject` titles the pull request, and the branch's name
titles it where the submit gives none. It waits for its conditions like
any other, and sluice submits it once they hold:

```console
$ wt submit main --subject "Batch review cards"
pushed batch-review · opened #41: Batch review cards
waiting: needs 1 review, qa/approved
  forecast: merges without conflict behind #38, #40
queued behind #38, #40
testing main + #38 + #40 + #41
testing main + #38 + #40 + #41 · 2 tasks to run, 0 replayed
passed · landed as b7d2e94
```

A waiting submit prints each state the pull request reaches, following
sluice's host process while it waits, and exits as any submit does. On a pull
request, `#41` is the pull request's number. `wt submit` removes the
worktree only when the pull request lands; sent back, the worktree
stays for the fix.

`--no-wait` returns once the pull request is open and sluice has read
it once, and exits `0`. When its conditions held then, sluice queued it
at the commits pushed. The submit answers with that submission, as a
submit that doesn't wait does on any branch, with the pull request
beside it:

```console
$ wt submit main --no-wait --subject "Batch review cards"
pushed batch-review · opened #41: Batch review cards
submission 12 to main from #41 · nothing queued ahead
$ wt submit main --no-wait --json
{"id":12,"branch":"main","ahead":[],"pull_request":41}
```

Otherwise it answers with the pull request and what it waits for, as
`wt submit main --status` reports it later: under `--json`,
`{"pull_request":41,"branch":"main","state":"waiting: needs 1 review"}`,
with no `id`.

A pull request a submit opened is the agent's. When its submission
leaves the queue without landing, blamed, in conflict, withdrawn, or on
commits no submit pushed, sluice marks it a draft, and no round takes it
again in the agent's name. The next submit marks it ready for review at
the commits it pushes, the same ones included, and sluice takes them
like any others. After a run the time limit ended, with no new commits:

```console
$ wt submit main
pushed batch-review · updated #41: Batch review cards · marked ready for review
queued behind #38
testing main + #38 + #41
testing main + #38 + #41 · 2 tasks to run, 0 replayed
passed · landed as c41f0a2
```

A submit of new commits while the pull request is still in the queue
withdraws its submission there, and the new commits wait for their
conditions; the submit says `withdrew its submission 12`.

## `sluice branch`

sluice makes and deletes the branches it lands on, so nothing else has
to write them. Each takes the branch's lock, so no round acts on it
meanwhile, and each is one move from what it read, refused when the
branch was elsewhere.

`sluice branch create <branch> --from <other>` makes the branch at the
other branch's current revision, fetched from its remote when it has
one. Both must be branches sluice lands on: the policy at that revision
gives each a gate, and the new branch lives where that policy's
`remote` says. A branch already there is refused, and so is a name
whose records still queue submissions.

`sluice branch delete <branch>` deletes it, only when:

- no submission on it is still queued or under test;
- no landing on it is unfinished;
- its revision is in `main`'s history, or the history of the branch
  `--into` names.

The deletion is leased on the revision it read, so a landing since,
from any host, refuses it. Its state stays: `sluice status <branch>`
still answers, its first line `<branch> is gone`, with its queue and,
with `--all`, every submission it had, and `sluice overview` names it
as gone.

```console
$ sluice branch delete feature/payments
refused: #12 still queued on feature/payments; wait for them to land, or withdraw them
```

A branch is held when the revision a round reads it at lacks the last
landing sluice recorded there. A hand that moved it back does that, and
so does a branch that landed in this host's own refs gaining a `remote`
whose copy is older. While it is held, no combination is formed and
nothing lands; tests already running go on, and their results are read
once the hold ends. `sluice step`, `sluice status <branch>`, `sluice
overview` and a waiting submit or `sluice wait` say so, naming both
revisions:

```console
$ sluice step feature/payments
held: feature/payments reads 10e4791, which lacks the landing c43609b; put the landing back on feature/payments, or run sluice branch accept feature/payments 10e4791
```

The hold ends by itself on the first round that reads the branch at a
revision holding the landing again, as when the landing is pushed back
to the remote. A commit on top of the landing never holds the branch:
the queue is formed on it as it stands.

`sluice branch accept <branch> <revision>` is the other way out: the
operator accepts the revision read as the one the branch stands on,
naming it as `held` names it, whole or by seven characters or more. It
is logged, every later read must contain it, and the next round forms
the queue on it. It moves no branch, and is refused unless the branch
is held at that revision and still read there. A branch sluice makes
stands on the revision it was made at, so a branch deleted and made
again owes nothing to the landings of the one deleted.

```console
$ sluice branch accept feature/payments 10e4791
accepted feature/payments at 10e4791a2b5c6d7e8f90a1b2c3d4e5f6a7b8c9d0, which lacks the landing c43609b8e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6; the next round forms the queue on it
```

All three exit `0` when done, `1` when a branch they name doesn't exist,
and `2` on a refusal. A round that holds a branch is no failure:
`sluice step` exits `0`, saying `held`.

## `sluice forge check`

Checks what the host's
[`[forge]`](/sluice/reference/configuration/#forge) table needs for a
branch whose `remote` is on GitHub. The key reads and the app makes a
token for the repository. The app may write the repository's contents
and its workflows.
Every ruleset on the branch lets the app through and nobody else. The branch is the one checked out where it
runs unless it is named. It says each thing it checked, and exits `1`
when any fails, naming which.

GitHub shows who may bypass a ruleset only to the ruleset's admins, so
the check reads the rulesets with your own token: `GH_TOKEN`,
`GITHUB_TOKEN`, or the one `gh` is signed in with. Everything else it
reads as the app.

```console
$ sluice forge check main
ok      [forge] key /home/me/.config/sluice/app.pem reads
ok      app 123456 makes a token for acme/widgets
ok      the app may write acme/widgets's contents
ok      the app may write acme/widgets's workflows
failed  ruleset "main" on main lets more than the app through: RepositoryRole 5
```

Where `[forge]` names a `credential` command in place of a key, the
check runs the command and reads what GitHub shows the token's holder.
The command gives a token for the repository. The token is the app's
that `app_id` names, by the bot GitHub says holds it. The token may
push, by GitHub's answer to the first thing git asks for a push. Then
it checks the rulesets as with a key. GitHub shows a token's other
grants, workflows, checks and pull requests, to nobody holding only the
token. The check leaves them to wherever the app's key is kept.

```console
$ sluice forge check main
ok      [forge] credential /opt/forge/bin/token-helper --app sluice gives a token for acme/widgets
ok      the token is app 123456's
ok      the token may push to acme/widgets
ok      ruleset "main" on main lets only app 123456 through
```

[Protecting main on GitHub](/sluice/guides/github-app/) sets
it up.

## `sluice set-aside`

sluice records nothing under a test's key for four failures it tells
apart on its own, so none of them needs setting aside:

- a run whose program never started;
- an exit `[tests] not_run` names;
- a run whose executor reports refusing it something it needs;
- a test the time limit ended.

A host failure none of these catches still sits under the test's
key, and a later pass makes the key read as flaky. On a branch whose
`flaky` is `"fail"`, the same inputs then never pass. The operator who
finds the cause on the host sets the fail aside:

```console
$ sluice set-aside cli:tools --at 2026-10-17T13:05:32Z --reason "OrbStack was down"
set aside cli:tools 7272335f33f8 recorded at 2026-10-17T13:05:32Z on main 79b832d: OrbStack was down
no gate reads it from now; its line stays in the results
```

`--at` is when the result was recorded, as a flaky failure names it.
`--hash` gives the start of its key when two results of the test were
recorded at once. The result's line stays in `results.jsonl`. The
set-aside, with who made it and why, is kept in `set-aside.jsonl` beside
it, and no branch reads the result again. An agent never sets aside a
result; the Claude Code plugin refuses it, and tells the agent to bring
the operator what it found instead. Under `--json`, the set-aside's
record is printed.

Each set-aside marks a result sluice recorded when it should not have.
Its reason says what the host was doing, so a failure sluice could have
told apart on its own can be taught to it.

## `sluice wait`

`sluice wait <id>` waits for a submission's outcome as a waiting submit
does. It follows sluice's host process, which runs the rounds, starting
it where none runs, and runs none of its own; it ends with the same
outcome and exit. A round the host process could not run on the
submission's branch is said once on stderr, in its words, and the wait
goes on. A submission whose branch took its work is reported landed,
whatever the round failed after it. A submission already decided is
reported at once, and nothing runs. `sluice wait <branch>` waits for this worktree's last submission
to that branch, the one `sluice submit <branch> --status` reports.

sluice finds a submission by its number on any branch it can read,
whatever other branch it cannot. A branch whose policy, revision or
state sluice cannot read holds up no wait on another branch. When the
number is on none of the branches sluice could read, and some branch
could not be read, it says so and exits `3`. It names each branch it
could not read, with the words its read was refused with. Whether the
submission exists is then not known, so it never says there is no such
submission. Under `--json`, it prints the error document, kind
`internal`. `sluice status <id>`, `sluice amend` and `sluice withdraw`
find a submission, and say they could not, the same way:

```console
$ sluice wait 250
submission 250 is on none of the branches sluice could read
feature/cache could not be read: [checkout] declares a cache, which sluice no longer keeps: …
```

A number names a submission only where one branch holds it. A branch
whose state is on its remote numbers its submissions from the remote's
count. A branch in this host's own files numbers them from the host's,
so two branches can hold one number. A wait on such a number is refused, and
runs no round. It names each branch that holds the number, then each
branch it could not read, with why, and exits `64`, since its `2` is a
conflict. `--branch <branch>` names the one meant, and the wait then
reads that branch alone. `sluice status <id>`, `sluice amend <id>` and
`sluice withdraw <id>` are refused the same way, exit `2`, and take
`--branch` too. Under `--json`, the error document's `next` gives the
command once with each branch:

```console
$ sluice withdraw 26
refused: submission 26 is on more than one branch: feature/a, feature/b; name one with --branch, as sluice withdraw 26 --branch feature/a
$ sluice withdraw 26 --branch feature/b
withdrew #26
```

With `--timeout`, as `90s`, `9m` or `1h30m`, it returns once that long
has passed and exits `5`, with the submission as it stands. The
submission stays in the queue. An agent whose tool calls end before a
result comes submits without waiting, then waits in steps, for as long
as `sluice wait` exits `5`:

```console
$ sluice submit main --no-wait --json
{"id":12,"branch":"main","ahead":[]}
$ sluice wait 12 --timeout 9m --json
```

Under `--json`, it prints one document however it ends, out of time
and stopped with Ctrl-C included: the submission's record, as `sluice
status <id> --json` prints it but for its `notice`. Read the outcome
from its `state` and the exit. A landed or withdrawn submission leaves `sluice status
<branch>`'s listing, so searching that listing for an outcome never
finds one.

Without `--json`, it prints the submission's state as it changes and,
while its combination is under test, how far it has got:

```console
$ sluice wait 12
#12 queued
testing main + #12 · 3s of 45m
testing main + #12 · 7/12 tasks · running sluice:e2e 25m03s, site:prose 40s · 26m10s of 45m
testing main + #12 · 12 tasks to run, 0 replayed
passed · landed as b7d2e94
```

A submission blamed for a required test that a task it depends on
failed names the task failed, then the test it failed, and where the
task's log is kept. The test is never named as not run and has no log,
since it never ran. Read the task's log and fix the task:

```console
$ sluice wait 14
#14 queued
testing main + #14 · 3 tasks to run, 0 replayed
failed: api:integration
  root:integration failed by api:integration, a task it depends on
  on main + #14 (e20a6f9)
  log of api:integration: .git/sluice/.logs/main/c4/api:integration.log
the agent working in cards fixes it there and submits again
```

Under `--json`, the record's `failed` holds both: the task with its
`hash` and `log`, and the test with `failed_by`, as [the submission's
record](/sluice/reference/state/#a-submission) says.

A wait reports its own submission, and never another's outcome as its
own. Its own is the number given, the submission a waiting submit made,
or, for `sluice wait <branch>`, the worktree's last. It's landed once
its branch holds its work, by whatever landing took it, its own queue's
or another's: `landed as 4e1a07c: main holds its work, by a landing not
its own`, exit `0`, with `landed_elsewhere` in its record. A submission
withdrawn though its work landed still reads withdrawn in `sluice
status <id>`, and the wait reports it landed all the same.

A later submission from its worktree, or its pull request, may carry
its work, as when its session submits again on top of it. The wait then
says so and follows that one, printing how far that one has got:

```console
$ sluice wait 12
#12 withdrawn by resubmission: #13 carries its work, so this wait follows it
testing main + #13 · 3s of 45m
landed as b7d2e94: main holds its work, by #13's landing
```

It ends with its own submission all the same. It's landed once `#13`
lands, exit `0`. It's withdrawn, exit `4`, once `#13` is blamed, in
conflict, or withdrawn with nothing carrying its work on, or once the
pull request is closed or sent back: `#12 withdrawn by resubmission:
#13, which carried its work, failed`, then `sluice status 13`. Out of
time or stopped, it prints `still waiting: sluice wait 12`. Under
`--json`, its document is its own submission's record, with
`carried_by` naming what it followed and how that stands:
`{"id":13,"state":"landed"}`, or `{"pull_request":41,"state":"closed"}`
for a pull request with no submission on this host. The outcome of
what it followed is that submission's own: the wait hands it over to no
one, and it reaches its session as any outcome does.

A pull request read closed is read against its branch as a round reads
it, from its remote, before the wait calls it closed without landing.
One another queue landed is landed, exit `0`, even where no command on
this host has fetched the branch since. A branch it can't read ends the
wait as one that couldn't read, exit `3`.

To a branch with pull requests, `sluice wait <branch>` follows the
worktree's pull request as a waiting `sluice submit` does. Once sluice
has queued the pull request at its latest commits, it follows that
submission as `sluice wait <id>` does: out of time or stopped, it
prints the submission's record under `--json`, and `still waiting:
sluice wait 12` without. Until then, its document under `--json` is
the one `sluice submit --status --json` prints,
`{"pull_request","branch","state"}`, the state saying what the pull
request waits for. Once its submission is decided, it prints the
submission's record. Its own submission is the first it reads of the
pull request. When sluice takes the pull request in again on a new
head, as when the branch moves under it, the wait follows the new
head's submission and ends with its own, as above.

When another host takes the branch's pull requests in, the wait takes
none in and runs no rounds of that branch. It follows the pull request
on GitHub instead, reading it and sluice's check on its head at most
once in a quarter of `[forge] discovery`. It prints the check's title
and summary as they change. Above them it prints where the pull request
stands, again only when that changes:

```console
$ sluice wait main
#41 is submission 1406 on b
queued behind #38
  In main's queue; it lands once its combination passes.
```

The submission and the host after `is` are what sluice's check on the
pull request's head names. Where another host now keeps `main`, as this
host last fetched main's state, the words name it too, `#41 is
submission 1406 on b; c keeps main`: the two differ between a new
host's taking `main` after the last one's lease and its first post.
Before the host takes the pull request in, it reads `#41 waits to be
taken in by b`. Where no host keeps `main` yet, it says so.

Its documents under `--json` gain two fields: the one `--status`
prints, the one a `--no-wait` submit prints and the one at its timeout.
`taken_in_by` is `{"host":"b"}`, the host that keeps the branch, and
`null` where no host keeps it yet. `queue` is the fields of the check's
[`sluice:queue` block](/sluice/guides/pull-requests/#what-sluice-posts),
and `null` where the check carries none:

```json
{"pull_request":41,"branch":"main","state":"queued behind #38","taken_in_by":{"host":"b"},"queue":{"host":"b","taken_in":true,"submission":1406,"state":"queued","ahead":[{"submission":1405,"pull_request":38}],"waits_for":[]}}
```

It reports what GitHub shows with the words
and exit of a submission's outcome. Merged is `passed · landed as
b7d2e94`, exit `0`. Sent back is as the check names it, `failed: …`
with exit `1` or `conflict: …` with exit `2`. Closed without landing is
closed. Where the check's summary carries its `sluice:queue` block,
the block's `state` decides: `failed` or `conflict` is sent back and
`landed` is landed, even while GitHub still shows a run an earlier
failure completed. A check with no block, as an older sluice posts it,
is sent back when it failed. Under `--json` it prints
`{"pull_request","branch","state","title"}`, the state `landed`,
`failed` or `conflict`.

While the pull request is open, the wait reads where it stands from the
words of sluice's check, and never from the color GitHub keeps for the
check. Once a check has failed, GitHub shows it failed for good, while
sluice posts its new words on it as the pull request is held, taken in
again and tested. A pull request held for the next submit, its check
saying `waiting: the next submit`, is sent back while it is a draft,
with exit `1`. Once a submit or a person marks it ready for review, it
waits for the host that takes the branch's pull requests in to take it
in again. The wait then prints `waiting to be taken in by` and that
host's name, and goes on following it.

On a host where another takes the branch's pull requests in, `sluice
submit <branch> --status --json` prints a pull request landed or sent
back as it prints a decided submission's record. It names its `state`,
`landed`, `failed` or `conflict`, the check's `title` and `summary`, and
its `head`, `draft` and `sends`. It exits `0`, as `--status` does
wherever it finds the worktree's pull request.

A submission that landed beside the queue ends with `landed as 4e1a07c
beside the queue: no required check reads its changes`, exit `0`, and
under `--json` its record has `"beside": true`. While one waits behind
another submission though no check reads it on the branch as it is,
its line says `waits behind #13: its checks read this change` or
`conflicts with #13's work`, and its record has `"beside": {"behind":
13, "why": "read"}` or `"why": "conflict"`.

A wait in the worktree a submission came from, or a pull request was
opened from, reports that submission's outcome to the session working
there, so it hands the outcome over once its report is written, as
[`sluice inbox`](#sluice-inbox) does, and the outcome stays owed until
it is confirmed. `--confirm` confirms it on receipt, for a caller that
keeps what it is given. Under `--json`, the record it prints carries its
`delivery` as the wait leaves it. A wait on another worktree's
submission hands nothing over, and nor does one that ends out of time
or stopped. A submission by name came from no worktree, and nor did a
pull request from a branch sluice lands on, so a wait on either hands
nothing over, wherever it runs. In a bare repository or a git directory,
which has no files checked out, a wait hands over only what one in a
worktree on the branch its `HEAD` names would. Where it runs in no herdr
pane, the log's `handed` line names the agent there, `SLUICE_AGENT`,
else the branch. An outcome already confirmed is reported as it stands.
When the wait can't record the hand-over, it says so on stderr, and
`sluice inbox` hands the outcome over.

## `sluice inbox`

`sluice inbox`, run in a worktree, hands over each outcome still owed to
the worktree's submissions: decided, and not yet confirmed by the agent
working there. It hands over every submission from the worktree's
branch, on any branch, that landed, was blamed or conflicts. A pull
request opened from that branch counts as the branch's where no
`[branches]` table names the branch. No worktree pushes a branch sluice
lands on, so a pull request from one is no worktree's, and nor is a
[submission by name](#submitting-a-branch-by-name): no `sluice inbox`
collects either. A withdrawn submission, and one still queued or under
test, is not collected. `--worktree <path>` collects for another
worktree.

In a bare repository or a git directory, which has no files checked
out, `sluice inbox` collects only what a worktree on the branch its
`HEAD` names would, even where that branch has no commit yet. Where its
`HEAD` names no branch, it collects nothing.

An outcome is owed until the agent confirms it. The notice a round types
into the session's herdr pane, a `sluice wait` or waiting submit in the
worktree that reports it, and `sluice inbox` each hand it over, and none
of them confirms it: a session that stops before it acts on an outcome
is handed it again by its next `sluice inbox`. Each hand-over is a
`handed` line in [the branch's log](/sluice/reference/state/#the-log),
naming where it reached: the herdr pane where the command runs in one,
else the worktree's path, else, in a bare repository or a git directory,
the agent, `SLUICE_AGENT`, else the branch. Once the agent has acted on
an outcome, `sluice confirm <key>` confirms it, and no inbox hands it
over again. A blame or a conflict submitted again from the same worktree
is confirmed by that submit, since submitting again acts on it.

A session that starts again, after a restart or on another host, runs
`sluice inbox` once, in place of keeping a `sluice wait` for each
submission it made. So does a tool that submits with `--no-wait` and
never waits: it runs `sluice inbox` when it next looks.

```console
$ sluice inbox
[sluice] #12 landed on main as b7d2e94.
testing main + #12 · 12 tasks to run, 0 replayed
passed · landed as b7d2e94
Confirm it once you have acted on it: sluice confirm 12:landed

[sluice] #13 failed on feature/cards: cli:test. sluice wait 13 shows why.
testing feature/cards + #13 · 3 tasks to run, 0 replayed
failed: cli:test
  on feature/cards + #13 (e20a6f9)
  log of cli:test: .git/sluice/.logs/feature/cards/c4/cli:test.log
the agent working in cards fixes it there and submits again
Confirm it once you have acted on it: sluice confirm 13:failed
```

Each outcome opens with its notice, the one line a round types into the
pane, then what `sluice wait <id>` prints for it, then a last sentence
naming its key. An outcome nobody confirmed within the host's
[`[deliveries] expire_after`](/sluice/reference/configuration/#deliveries),
a week where unset, has expired, and is still owed: its last line opens
`Expired at 2026-11-15T10:02:00Z, still owed.` A blank line comes
between outcomes, and nothing at all is printed when nothing is owed.

`--confirm` confirms each outcome on receipt in the same call, and
prints no line naming its key: for a caller that keeps what it is given
for readers of its own. `--new` hands over only the outcomes nothing has
handed over yet, no notice typed into the pane and no inbox or wait
that printed them: for a harness that collects on each new outcome.

While it prints and records a branch's outcomes, it holds that branch's
lock, so no round pushes the same outcome into the pane meanwhile. It
prints before it records: one stopped between the two leaves the
outcome owed with no hand-over by it, and the next `sluice inbox` hands
it over again in the same words.

Under `--json`, it prints one document. `outcomes` holds each outcome
as the submission's record that `sluice wait <id> --json` prints, with
`delivery` as this run leaves it, and `notice`, the one line. It is `[]`
when nothing is owed:

```json
{
  "outcomes": [
    {
      "schema": "sluice/submission@1",
      "id": 12,
      "branch": "main",
      "head": "4f0c1d2e9a",
      "from": { "agent": "cards", "worktree": "cards",
                "path": "/home/me/wt/cards" },
      "submitted": "2026-10-28T09:12:03Z",
      "after": [],
      "priority": 0,
      "attempt": 1,
      "state": "landed",
      "tested": { "combination": "9ab3d07c11", "on": "c41e8f2d90", "submissions": [12] },
      "result": "passed",
      "landed_as": "b7d2e94f03",
      "notice": "[sluice] #12 landed on main as b7d2e94.",
      "delivery": {
        "key": "12:landed", "reader": "cards", "state": "owed",
        "recorded_at": "2026-10-28T09:40:12Z", "expires_at": "2026-11-04T09:40:12Z",
        "handed": [{ "by": "inbox", "at": "2026-10-28T10:02:41Z" }],
        "confirmed": null, "known": true
      }
    }
  ]
}
```

A script may rely on these fields of each element: `id`, `branch`,
`state` (`landed`, `failed` or `conflict`), `notice`, `delivery`,
`from.worktree` and `from.path`. A landed one has `landed_as`. A failed
one has `result`, `failed` or `timed out` when the time limit ended it,
and `failed`, each with its `task` and, where it was kept, its `log`. A
conflicting one has `conflict`, with its `paths` and `with`, each with
the `id` of a submission it conflicts with. A later sluice keeps these
names and what they mean, and only adds fields.

`delivery` is where the outcome stands: its `key`, its `reader` (the
worktree's branch, or `pull/<n>` for a pull request), its `state`
(`owed`, `expired` or `done`), when it was recorded and when it expires,
each time it was `handed` over and how, `confirmed` (when, or `null`)
and `known` (whether anything has been confirmed under its reader's
name). `sluice status <id> --json`, `sluice wait --json` and a waiting
submit's give it too. One confirmed and let go of, once its submission
left the queue, is `{"key": "12:landed", "state": "done"}`. A submission
by name, a withdrawn one and one not decided have none.

A branch whose records it cannot read stops no other: it hands over
what it could read, says which branch it could not and why on stderr,
`sluice: could not read feature/cards's records: …`, and exits `3`. Under
`--json`, the document adds `unread`, `[{"branch","error"}]`, present
only then.

It exits `0` whether anything was owed or not, and `3` when a branch's
records could not be read or sluice itself failed. A worktree on no
branch has submitted nothing: `sluice inbox` there prints nothing, or
`{"outcomes":[]}` under `--json`, and exits `0`.

## `sluice confirm`

`sluice confirm <key>...`, run in a worktree, confirms each outcome its
key names, on whichever branch holds it, once its agent has acted on it.
The key is the one its notice and `sluice inbox` give it: `12:landed`,
`13:failed` or `14:conflict`. A confirmed outcome is handed over no
more, by any inbox, notice or wait.

```console
$ sluice confirm 12:landed
confirmed 12:landed (#12 landed on main)
$ sluice confirm 12:landed
12:landed was confirmed at 2026-10-28T10:40:12Z
```

Confirmed again, it says when it first was, writes nothing, and exits
`0`. An expired outcome is confirmed as an owed one. A key no branch
holds is not found, with `sluice inbox` to list the keys, and exits `1`.
A key whose outcome is owed to another worktree is refused, naming it,
and exits `2`: `sluice confirm <key> --as operator` confirms it for that
worktree, as for one removed before it confirmed, and the log names the
operator as who confirmed. A branch it could not read is named on
stderr, and a key not found exits `3`, since it may be there. Given
several keys, it answers each in order and exits with the worst: `3`
over `2` over `1`.

Under `--json`, it prints one document, `confirmed`, each key's `key`,
`branch`, `reader`, `at` and `first`, `true` where this call confirmed
it. A key refused or not found gives the error document, with its
`kind` and `next`.

## Exit codes

| exit | meaning |
|---|---|
| `0` | landed, whatever failed after the landing: what failed is on stderr, after `sluice: after the landing:` |
| `1` | failed: this submission was blamed |
| `2` | a conflict, or a refusal |
| `4` | withdrawn, and none of its work landed |
| `5` | `sluice wait --timeout` ran out; the submission stays queued |
| `130` | stopped with Ctrl-C; the submission stays queued |

A submission has landed once its branch holds its work, and nothing that
fails after that changes it. A waiting submit or `sluice wait` whose
round moves the branch and then fails, releasing the combination's
checkout or handing the outcome over, prints the landing and exits `0`. What
failed is on stderr, on its own line:

```console
passed · landed as b7d2e94
sluice: after the landing: the release command of … ended exit status: 1: usage: scripts/preview up
```

Under `--json`, stdout holds the submission's record alone, its `state`
`landed`. When the round stopped before recording the landing, the
record is the one the next round writes.

Every command, not only submit, exits `3` when sluice itself fails (a
repository it cannot open, a record it cannot read), with the cause on
stderr. A waiting submit or `sluice wait` does so only while the
submission has not landed. `sluice wait`, `sluice status`, `sluice
amend` and `sluice withdraw` find a submission by its number. Each exits
`3` as well when the number is on none of the branches it could read
while some branch could not be read, and names each such branch with
why. Each says there is no such submission, exit `1`, only when every
branch was read. Each refuses a number more than one branch holds,
naming each branch, until `--branch <branch>` names one: `sluice wait`
exits `64` for it, and the others `2`. A round sluice's host process
could not run never ends a wait (see [Who runs the rounds](#who-runs-the-rounds)). A command exits `64` when it cannot read its
command line: an unknown command, flag or value, or a missing argument. `64` is kept apart from
`2`, so a mistyped flag is never taken for a conflict. `--help` and
`--version` exit `0`.

sluice tries a call to a remote again when the remote never answered it:
a connection not made, a server error such as a 5xx, a token refused.
That holds for every call, a git fetch or push or a call to GitHub's
API, and sluice pauses longer before each try. It tries for five minutes in a waiting submit or
`sluice wait`, ending sooner at `--timeout` or Ctrl-C, and for thirty
seconds in any other command. A push whose answer was lost is never
pushed again: sluice reads the branch on the remote, and what it holds
says whether the push was made. A check run, a comment or a pull request
whose answer was lost is looked for on GitHub before it is made again,
so it is never made twice. A workflow's run is never started twice. A
run whose start went unanswered shows as started in the branch's log,
with GitHub's words, and sluice starts it again only when no job for it
appears.

A waiting submit or `sluice wait` whose read GitHub still cannot answer
once those five minutes are spent exits `3` as well, saying it could not
read and how long it tried, apart from any outcome:

```console
could not read #77: sluice wait main
sluice: could not read #77 for 5m03s, so whether it landed is not known: GET /repos/o/r/pulls/77: 503 Service Unavailable
```

Under `--json`, stdout holds the error document, with that command in its
`next`. Whether the submission landed is not known then: only its record
says, and the command in `next` reads it again. An outcome only ever comes
from the submission's record.

With `--json`, a command that cannot do what it was asked still prints
JSON on stdout, one document in place of the one it would have printed:

```json
{"error":{"kind":"refused","message":"no branch feature/none","next":["sluice branch create feature/none --from main","sluice overview"]}}
```

`kind` is `refused` (exit `2`), `not_found` (exit `1`) or `internal`
(exit `3`, with the cause on stderr as well). `next` holds the commands
that move it on, and is empty when there are none.

The smaller commands print one line each under `--json`, and anything
else they say goes to stderr:

| Command | Document |
|---|---|
| `sluice step` | `{"branch","outcome","released","previews_released","releases"}`: `outcome` is `stepped`, `busy`, `led`, `contended` or `held`; `released` and `previews_released` name the worktrees and previews the round freed because they were gone or released because their command failed, and `releases` each combination's checkout whose release failed, was held or found gone, with `release` (`failed`, `held` or `gone`), `failed` and `tries`. A worktree or checkout found gone carries `ran`, whether its stored release ran, and `failed`, what it printed when it failed; a preview found gone carries them under `gone`. A held round adds `held`, with `read`, `lacks` and `since` |
| `sluice branch` | `{"branch","created","from"}`, `{"branch","deleted","into"}` or `{"branch","accepted","lacked"}` |
| `sluice amend` | `{"id","after","priority"}`, as the submission now stands |
| `sluice withdraw` | `{"id","state":"withdrawn"}`, with `"already":true` when it was withdrawn before |
| `sluice set-aside` | `{"schema","at","task","hash","run_at","branch","combination","reason","by"}`, the record kept |
| `sluice release` | `{"worktree","released"}`, or `{"preview","released"}` for a preview; `released` is `false` when the policy names no release command, the folder is in no worktree, or the release failed. With `--accept-failure` it adds `accepted`, and a release that failed adds `failed`. For a held checkout, `{"place","released","accepted","failed"}` |
| `sluice protect` | `{"protection":"installed","changed","held"}`: `changed` is `false` when the hook was in place already, and `held` lists each worktree that has checked out a protected branch; with `--remove`, `{"protection":"removed","changed"}` |
| `sluice forge check` | `{"branch","ok","checks"}`, each check with its `ok` and `message` |

A submit following a pull request exits `1` as well when the pull
request is closed without landing.

`sluice preview` exits `0` once its command has started, or with
`--wait` once the preview is up; `1` with `--wait` when its command
failed; `2` when the revision or the branch is refused; `130` when
stopped while it waits.

`sluice release` exits `1` when the release it ran failed, saying what
failed, or when the policy that names its release command could not be
read, naming the branch, and `0` otherwise, nothing to release included.

`sluice exec` exits `0` when every test passed, `1` when one failed, was
not run, could not run or timed out, `2` on a refusal, and `130` when stopped with
Ctrl-C, which ends the run. With a command, it exits with the command's
code.

`sluice equip` exits `0` when every step passed, or there was none, `1`
when one failed, and `2` on a refusal.

## Who runs the rounds

A round is one pass of sluice's work for a branch: read the queue, start
tests if there is room, read finished tests, blame a failure, land what
passed, write the queue back. A lock lets only one run at a time for a
branch.

- **On one machine**, sluice's own host process runs them. The first
  command that needs the host's queues moved, a submit among them,
  starts it, and it ends once no repository it holds has work in play
  and nothing follows it: nothing is installed or kept running
  ([ADR 0093](/sluice/decisions/0093/)). It holds every repository on
  the host that has sluice, each in a task of its own, so a branch one
  repository cannot read holds up no round of another, and it holds the
  host's bound on runs in one place. A waiting `sluice submit`,
  `wt submit` or `sluice wait` follows it and runs no round, and
  `sluice step` asks it for one round of a branch. A command that finds
  work in play and no host process answering, `sluice status` among
  them, starts it again, so a crash or a sleeping laptop loses nothing.
  For a branch with pull requests, each round first reads its open pull
  requests on GitHub.
- **On many hosts**, one host at a time keeps a branch whose state is on
  its remote: whichever host's round wrote its state last. A round on
  another host reads `led` and leaves the branch alone. Once the state
  has stood unchanged for that host's
  [`[peers] lease`](/sluice/reference/configuration/#peers) with work to
  do, the next round there takes the branch, and the former host writes
  nothing more for it. Each host measures the lease by its own clock,
  never by comparing clocks. A submission written to the remote goes on
  with its submitter's host switched off: any other host lands it.
  [Executors and systems](/sluice/guides/executors-and-systems/) sets
  one up.

The host process passes by a branch it cannot read, one whose policy,
revision or state it cannot read or whose round fails, and goes on with
every other branch's round. It says so once in its log, `host/log` in
[sluice's state](/sluice/reference/state/), with the words it was
refused with, and again only when those words change or when the branch
is refused again after a pass that read it. A wait on that branch says
it once on stderr, and goes on waiting:

```console
sluice: feature/cache: [checkout] declares a cache, which sluice no longer keeps: …
```
