---
title: Running checks on GitHub Actions
description: "Let GitHub Actions take the runs no host of yours has room for, or a system none of them runs: one workflow run per run, its checks in one job and its signature in another."
sidebar:
  order: 8
---

With [several hosts sharing a queue](/sluice/guides/executors-and-systems/),
a run waits until a host of its system has room. A pool on GitHub
Actions takes the runs no host takes: a burst your hosts can't keep up
with, or a system none of them runs, such as macOS on a team of Linux
boxes. sluice starts one workflow run for each such run, and the
workflow's own identity signs its results, so they count as a host's do.

You need sluice's [GitHub App](/sluice/guides/github-app/) set up for the
branch, and a policy with `[signers]`, so the queue lives on the remote
where a workflow can read it.

## Set it up

In a working copy of the branch the workflow runs from, usually `main`:

```console
$ sluice init actions
.github/workflows/sluice.yml: written
.github/workflows/sluice-pr.yml: written
.config/sluice.toml: [signers.forge] and [pools.forge] appended; a branch's `systems` names the systems its runs need, and `trust` names forge

Then, on GitHub:
  - give sluice's app the permission to run workflows: Actions, read and write
  - make an environment named `sluice` that only main may deploy from, holding the secret SLUICE_APP_KEY, the app's private key, and the variable SLUICE_APP_ID, its id
  - land these files on main, where the workflow runs from
```

It appends two tables to the policy, leaving your own lines as they
were:

```toml
[signers.forge]
github = { workflow = ".github/workflows/sluice.yml", ref = "refs/heads/main" }

[pools.forge]
github = { workflow = ".github/workflows/sluice.yml" }
signer = "forge"
runners = { linux-x64 = "ubuntu-24.04", macos-arm64 = "macos-15" }
```

The signer is the workflow itself, at the branch it runs from: a result
counts only when that workflow on that branch signed it. The pool names
the runner GitHub gives each system. Add `forge` to the `trust` of each
branch whose runs it may take, and name in its `systems` the systems
its runs need.

Then do what it printed on GitHub. In the app's settings, give it
**Actions: Read and write**, so sluice can start the workflow. In the
repository's settings, make an environment named `sluice`. Under
**Deployment branches**, choose the branch the workflow runs from
alone, so no other branch's workflow can read the app's key. Give the
environment the secret `SLUICE_APP_KEY`, the app's private key, and the
variable `SLUICE_APP_ID`.

`sluice forge check main` reads all of it back:

```console
$ sluice forge check main
...
ok      the signer forge verifies .github/workflows/sluice.yml at refs/heads/main
ok      .github/workflows/sluice.yml on main is as sluice init actions writes it
ok      the app may run acme/widgets's workflows
ok      only main may use acme/widgets's sluice environment
```

## Your hosts first

A pool takes a run only when no host could take it now: none of its
system has room, or none of its system is there at all. A host that has
room takes the run first, as before. Each workflow run uses the
repository's Actions minutes, and a macOS runner uses them faster than
a Linux one, so keep the hosts you have and let the pool take what they
can't.

A run the pool can't take either, with no runner named for its system,
waits for a host. When the branch's `timeout` passes with nothing able
to take it, the combination is not passed: no one is blamed, and the
combination is formed again.

## What a workflow run does

sluice claims the run, writes the claim in the branch's state, and then
starts the workflow, titled with the claim, `sluice pool/forge/12`. A
claim written and not started is started by the next round, and one
started and not marked is never started twice.

The run has two jobs:

- **run**, on the system's runner, installs sluice and the toolchain,
  starts the claimed run as a host would, and keeps its report. It reads
  the repository and nothing else: it holds no key, no identity and no
  secret, so nothing the combination's checks do can sign anything.
- **sign**, after it, whatever it came to, in the `sluice` environment.
  It runs none of the combination's code. It keeps only the results
  whose keys the combination's tree bears out, signs them with the
  workflow's own identity through GitHub's attestations, and writes the
  run ended in the branch's state.

The lead reads the run as running from the run job's own start, and
its time limit counts from there. A workflow run that ends with nothing
recorded, cancelled or failed before the sign job wrote, is a claim that
lapsed: its checks run again in a run of their own.

A signed result is checked on every host that reads it against the
roots sluice carries, with nothing fetched, by the workflow and the
branch the signer names. A result signed by any other workflow, or by
this one on another branch, counts for nothing.

## Pull requests

`sluice-pr.yml` runs each pull request's head on each of the pool's
systems, with no secret and no identity. `sluice.yml` signs what those
runs bear out, as it signs a claimed run, and adds the results to the
signer's runs. A combination that holds the same work reads them by
key: a check whose key passed on the head runs nothing again. A pull
request that edits `sluice.yml` runs its own copy of the workflow, from
its own branch, so nothing it signs counts.

A `pull_requests.checks` entry names checks other systems post. One
that names `sluice`, or one of sluice's own checks of a task, is
refused: a head's runs already count by key.

## With every host off

The pool takes runs; it doesn't run the queue. A round runs on a host
of yours, so with every host off a submission waits. Rounds run as a
job of their own are [designed, not built](/sluice/roadmap/).
