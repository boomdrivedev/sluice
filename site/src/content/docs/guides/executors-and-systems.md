---
title: Executors and systems
description: "sluice on many hosts: one queue on the git remote, hosts that sign the results they run, a box kept warm for the checks, and a test that must pass on Linux and macOS."
sidebar:
  order: 7
---

sluice on one host keeps each branch's queue in the repository's git
directory, and runs every check itself. That stays true until a policy
names signers. This guide moves a branch to many hosts: agents on any
machine submit to one queue, a box you keep running takes the checks,
and an agent can submit and switch its host off. No server takes part:
every host runs the same sluice, and the queue lives on the git remote.

## One host, as today

Nothing here changes a policy with no `[signers]` table. Each host keeps
its own queue, runs its own checks, and a branch's
[`trust`](/sluice/reference/configuration/#trust) lists executors, as
before.

## What moves to the remote

A branch with a `remote`, in a policy with signers, keeps its queue on
that remote under `refs/sluice/`
([state](/sluice/reference/state/#a-branch-whose-state-is-on-its-remote)).
Every host that submits to the branch reads and writes the same queue.
One host at a time keeps the branch, and another takes it over once the
queue has stood still for that host's
[`[peers] lease`](/sluice/reference/configuration/#peers) with work to
do. Runs are claimed: a host with room takes the earliest open run of a
system it runs, and a claim lapses once its host has shown no sign of
life for `[peers] alive`.

Because any host can write the queue, a result counts only when a
signer the branch trusts signed it. A record that only says it came
from a confined container counts for nothing.

## Signers

Give each host that runs checks an SSH key of its own, kept on that
host:

```sh
ssh-keygen -t ed25519 -f ~/.config/sluice/signing -N ""
```

Name it in that host's config, `~/.config/sluice/config.toml`, or where
the host's configuration is managed
([who writes it](/sluice/reference/configuration/#who-writes-it)):

```toml
[signing]
signer = "box"
key = "~/.config/sluice/signing"
```

List its public half in the policy, and trust it on the branches it
may decide:

```toml
[signers.box]
ssh = "exec@build-1 ssh-ed25519 AAAA..."
confined = true
systems = ["linux-x64"]

[signers.sessions]
ssh = ".config/sluice.sessions"   # one line per agent host's key
confined = false

[branches."feature/*"]
remote = "origin"
trust = ["sessions", "box"]

[branches.main]
remote = "origin"
trust = ["box"]
confined = true
```

`confined = true` on a signer is a claim you make for it: no agent can
reach the checkouts its runs use, or what they wrote. Say it only of a
host that runs no agents, where each run gets a fresh container.
sluice never works it out.
A branch with `confined = true` counts only results from signers you
said that of.

Agents' hosts sign as `sessions` here. Their results land work on the
feature branches, where reviewers trust them, and never on `main`, whose gate
trusts only the box. A host signs with its own key, and sluice on it
claims only runs of branches that trust its signer.

## A box kept warm

A standing executor is a host whose sluice is moving its queues: its own
process starts when any command of sluice's there finds work, or when
whatever wakes the host runs one. Its build cache stays warm between
runs, so it is the cheapest place for a check. Its config says what it
signs as, how many runs it holds, and which systems it runs:

```toml
[signing]
signer = "box"
key = "~/.config/sluice/signing"

[runs]
max = 4
systems = ["linux-x64"]
```

sluice's process on the box serves every branch with records in the
repositories it holds, those on the remote included. On a branch another
host keeps, it claims open runs it may take, runs them, signs what they
found and adds it to `refs/sluice/runs/box`. Where `[runs] systems` names
a system the box isn't, it claims no run there, and says so in its log.

Before it claims anything after coming back, a host releases the
checkouts it left behind. A run whose claim lapsed while it was away
has already gone to another host, and the late result is never
written.

## Choosing systems per check

A repository that ships for several systems names them, and a branch
says which each check must pass on:

```toml
[systems]
linux-x64   = { os = "linux", arch = "x86_64" }
macos-arm64 = { os = "macos", arch = "aarch64" }

[branches.main]
systems = ["linux-x64"]

[branches.main.systems_for]
"cli:build" = ["linux-x64", "macos-arm64"]
```

A pass on one system never stands for another. Each combination becomes
one run per system, each run holding only the checks that need it, and
a host takes only runs of its own system. A run no host can take
fails the combination when the branch's `timeout` passes, naming the
check and the system it never ran on. Name a system only for the checks
that need it: [Parallel and
batch](/sluice/guides/parallel-and-batch/#systems-multiply-runs) counts
what each costs.

## When no box has room

A run waits until a host of its system that the branch trusts has room,
and claims go earliest combination first. GitHub Actions as a pool of
machines that takes a burst, or a system no box runs, is [designed, not
built](/sluice/roadmap/).

## Watching it

`sluice overview` lists the hosts serving a remote, by their last sign
of life: the signer, the system, the runs held, the room left, and a
host whose sign of life has stood still past `alive`. `sluice step
<branch>` says `led` on a host that does not keep the branch, and still
takes its runs. `sluice status <branch>` reads the queue from the remote.
When the remote is out of reach, it shows the queue as last fetched and
says so.

## Moving a branch that has a queue

The landing that adds `[signers]` moves each branch with a remote. The
first host to take a branch pushes its own queue there as the start.
Another host whose queue wasn't first submits its queued work again, and
`sluice status <old id>` names the new one. Results recorded before the
move were nobody's signature, so each check runs once more under a
signer.
