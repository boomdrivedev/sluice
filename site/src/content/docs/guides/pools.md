---
title: Running checks on a pool
description: "Let a pool take the runs no host of yours has room for, or a system none of them runs: GitHub Actions, or a CodeBuild fleet of your own, one job per run, its checks in one step and its signature in another."
sidebar:
  order: 8
---

With [several hosts sharing a queue](/sluice/guides/executors-and-systems/),
a run waits until a host of its system has room. A pool takes the runs
no host takes: a burst your hosts can't keep up with, or a system none
of them runs. sluice starts one job on the pool for each such run, and
the pool's own signer signs its results, so they count as a host's do.

sluice has two pools:

- **GitHub Actions**, which runs each run as one workflow run, signed by
  the workflow's own identity. It suits a few runs a day, or a system
  such as macOS that none of your hosts runs.
- **A CodeBuild fleet** in your own AWS account, which runs each run as
  a build on a reserved-capacity fleet of warm instances, signed by a
  key in AWS KMS. It suits a queue whose every run goes to the pool.

Either needs sluice's [GitHub App](/sluice/guides/github-app/) set up
for the branch, and a policy with `[signers]`, so the queue lives on the
remote where a pool's job can read it.

## When a pool takes a run

A pool takes a run only when no host could take it now. None of its
system has room, or none of its system is there at all, or no host
signs as a signer the branch's `trust` names. A host that may take the
run and has room takes it first. A pool's runs hold no part of any
host's bound.

A gate that trusts the pool's signer, and a signer of your hosts that
may claim no system, sends the pool every run. Your hosts may run the
branch's queue, since the gate trusts their signer, and take none of its
runs.
That is how a branch runs every check on a CodeBuild fleet.

A run the pool can't take either, with no runner named for its system,
waits for a host. When the branch's `timeout` passes with nothing able
to take it, the combination is not passed: no one is blamed, and the
combination is formed again.

sluice claims the run, writes the claim in the branch's state, and then
starts the pool's job, under the claim, `pool/forge/12`. The next round
starts a claim written and not started, and never starts one twice.
The host that runs the queue reads the run as running from the job's
own start, and its time limit counts from there.
A job that ends with nothing recorded is a claim that lapsed: its checks
run again in a run of their own.

## GitHub Actions

In a working copy of the branch the workflow runs from, such as `main`:

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

Each workflow run uses the repository's Actions minutes, and a macOS
runner uses them faster than a Linux one, so keep the hosts you have
and let the pool take what they can't.

### What a workflow run does

The run has two jobs:

- **run**, on the system's runner, installs sluice and the toolchain,
  starts the claimed run as a host would, and keeps its report. It
  keeps the checkout the checks ran in for [your own
  steps](#your-own-steps-after-the-checks), then releases it after
  them, whatever they came to, and hands the report over. It reads the
  repository and nothing else: it holds no key, no identity and no
  secret of sluice's, so nothing the combination's checks do can sign
  anything.
- **sign**, after it, whatever it came to, in the `sluice` environment.
  It runs none of the combination's code. It keeps only the results
  whose keys the combination's tree bears out, signs them with the
  workflow's own identity through GitHub's attestations, and writes the
  run ended in the branch's state.

A signed result is checked on every host that reads it against the
roots sluice carries, with nothing fetched, by the workflow and the
branch the signer names. A result signed by any other workflow, or by
this one on another branch, counts for nothing.

### Your own steps after the checks

The run job leaves room for steps of your own after the checks, such as
pushing what the checks built to a shared cache. Write them between its
two `# sluice:` lines:

```yaml
      # sluice: your own steps, after the checks, in $SLUICE_CHECKOUT
      - name: Push what the checks built
        if: ${{ !cancelled() && env.SLUICE_CHECKOUT != '' }}
        working-directory: ${{ env.SLUICE_CHECKOUT }}
        run: ./scripts/push-cache
        env:
          CACHE_KEY: ${{ secrets.CACHE_WRITE_KEY }}
      # sluice: end of your own steps
```

They run in the checkout the checks ran in, which the job names as
`SLUICE_CHECKOUT`: `${{ env.SLUICE_CHECKOUT }}` as a step's
`working-directory`, or `cd "$SLUICE_CHECKOUT"` in its `run`. The
checkout still holds what the checks wrote. The job releases it after
your steps, whatever they came to. A step with no `if:` runs after any
run that kept its report, whether its checks passed or failed;
`if: ${{ !cancelled() }}` runs it even where the run kept none.

Your steps hold the credential you give them, never one of sluice's:

- **A secret of the repository's**, `${{ secrets.NAME }}`, allowed to
  write your store's cache and nothing else.
- **For a store on GitHub's registry**, the job's own token, given
  `packages: write` on the line directly after `contents: read` in the
  run job's `permissions`:

  ```yaml
      permissions:
        contents: read
        packages: write
  ```

  The token reaches the checks too, so keep the repository's other
  packages out of its Actions access in each package's settings. No
  other permission, environment or identity token belongs in the run
  job.

A pull request from a fork gets no secrets and a token that only reads,
so its run job's steps push nothing. `sluice-pr.yml`'s run job leaves
the same room as `sluice.yml`'s.

`sluice init actions` reads a workflow that differs only between those
lines, and in that `packages: write`, as it writes it, and
`sluice init actions --force` keeps them when it writes a newer
release's workflow. A step you added after the step that runs
`sluice job run` before this room existed would be written over: move
it between the lines first. A line between them that would reach past
the run job's steps, such as `environment:` at the job's own
indentation, is refused, by `sluice init actions` with `--force` or not
and by `sluice forge check`, which names the line.

### Pull requests

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

## A CodeBuild fleet

A CodeBuild pool runs on reserved-capacity fleets in your own AWS
account: instances kept warm, one fleet for each Linux system, `linux-x64`
and `linux-arm64`. A warm instance keeps the image layers and the
toolchain's store between builds, so a run starts without pulling them
again.

### Write the stack

In a working copy of the branch, write the stack's template:

```console
$ sluice init codebuild
.config/sluice-codebuild.yml: written, a fleet and a run project for linux-x64

Then, in AWS:
  - deploy it, as `sluice` or a name of your own:
    aws cloudformation deploy --template-file .config/sluice-codebuild.yml --stack-name sluice --capabilities CAPABILITY_IAM --parameter-overrides StartPrincipal=<the ARN the hosts that may run the branch's queue reach AWS as> AppId=<sluice's app id> MaxCapacity=<the branch's parallel> RunPolicies=<the ARNs of the managed policies your own commands after the checks need, comma-separated, or nothing>
  - fill the secret sluice-app-key with the app's private key:
    aws secretsmanager put-secret-value --secret-id sluice-app-key --secret-string file://<the app's key>
  - name the profile that assumes the stack's start role as [aws] profile on each host that may run the branch's queue
  - then `sluice init codebuild --stack sluice`, which names the pool in .config/sluice.toml
```

It writes a fleet and a run project for each system the policy's
`[systems]` names that CodeBuild runs on Linux, or for those
`--systems` names, and says which systems it leaves out. A template
that differs from what sluice writes is left as it is, with how it
differs, unless `--force`.

The template holds:

- each system's **fleet**, its base and maximum instances parameters,
  queuing a run past the maximum;
- each system's **run project** on its fleet, with no source and its
  buildspec inline;
- the **sign project**, on demand, in a small container;
- the **bucket** a run build hands its report to the sign build in;
- the **key** in AWS KMS, `ECC_NIST_P256` for signing, aliased
  `alias/<stack>-sign`, which only the sign build's role may sign with;
- the **secret** for the app's private key, empty until you fill it;
- the **rule** that starts the sign build when a run build ends;
- the **start role**, which the hosts that may run the branch's queue
  assume to start run builds.

### Deploy it

Deploy the template with the command it printed. `StartPrincipal` is
the ARN the hosts that may run the branch's queue reach AWS as, which
may assume the start role. `AppId` is sluice's app's id. `MaxCapacity` is the most
instances a fleet grows to, and `BaseCapacity` the instances it keeps
warm. `RunPolicies` names the managed policies your [own
commands](#your-own-commands-after-the-checks) need, none by default.
Fill the secret with the app's private key.

On each host that may run the branch's queue, name the AWS profile that
assumes the start role in the host's settings:

```toml
[aws]
profile = "sluice-start"
```

Every call sluice makes to the fleet uses that profile, whatever profile
the command that started sluice names. With no `[aws]`, sluice finds
credentials as the AWS tools do.

### Name the pool

Then name the pool in the policy, from the stack you deployed:

```console
$ sluice init codebuild --stack sluice --region us-east-1
.config/sluice.toml: [signers.codebuild], naming the key arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab, and [pools.codebuild] appended; a branch's `trust` names codebuild and the signer the hosts that may run its queue sign as, that signer's `systems = []` so it claims no run, and the branch's `systems` the systems its runs need
```

It reads the stack's key with your own AWS credentials, `--profile`
where you name one, and appends two tables, leaving your own lines as
they were:

```toml
[signers.codebuild]
kms = { key = "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab", public = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE..." }
systems = ["linux-x64"]

[pools.codebuild]
codebuild = { region = "us-east-1", sign = "sluice-sign" }
signer = "codebuild"
runners = { linux-x64 = "sluice-run-linux-x64" }
```

The signer is the key, by its ARN, and its public half, which every host
verifies a result with, asking AWS nothing. The pool names the stack's
region, its sign project and the run project for each system.

### Test side by side

A host of yours runs the branch's queue: its rounds, and the start of
each run on the fleet. A host runs the queue of a branch held on its
remote only when it signs as a signer the branch's gate trusts. A
branch whose `trust` names only the pool's signer has no host to run
its queue, and nothing happens. Name the signer the hosts that may run
its queue sign as, with `systems = []`, and trust it beside `codebuild`:

```toml
[signers.box]
ssh = ".config/sluice.boxes"   # one line per host's key
systems = []                   # it may claim no run

[branches.main]
remote = "origin"
trust = ["box", "codebuild"]
systems = ["linux-x64"]
batch = false
parallel = 8
```

A signer whose `systems` is empty claims no run and counts for no
result, whatever the run's system. So the hosts that run the queue take
none of the branch's runs, whatever executors they have, and the fleet's results
are the only ones that land anything. A host can't sign as `codebuild`:
its one key is the key in AWS KMS.

A fleet runs as many runs at once as it has instances, so test each
combination side by side at the fleet's size. `batch = false` tests each
combination as it forms, rather than waiting to test the deepest, and
`parallel` at the fleet's maximum keeps every instance busy. See
[Parallel and batch](/sluice/guides/parallel-and-batch/).

`sluice pool check main` reads the pool back from AWS:

```console
$ sluice pool check main
ok      main's queue is run by a host signing as box
ok      [pools.codebuild] is reached in us-east-1 as arn:aws:sts::111122223333:assumed-role/sluice-start/host-a
ok      sluice-run-linux-x64 runs linux-x64 on the reserved-capacity fleet sluice-linux-x64, LINUX_EC2
ok      sluice-run-linux-x64's buildspec is as sluice 1.2.3 writes it, cloning acme/widgets
ok      sluice-run-linux-x64 may take 90 minutes, longer than the branch's timeout of 45
ok      sluice-sign's buildspec is as sluice 1.2.3 writes it
ok      the key arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab is ECC_NIST_P256, SIGN_VERIFY, and its public half is the one [signers.codebuild] lists
```

It fails naming each that does not hold, a branch whose trust names no
signer a host signs as among them. Where the branch doesn't test side by
side at the fleet's size, it says so, failing nothing. Land the
policy, and the next round starts the first run on the fleet.

### What a run does

A run is two builds:

- **The run build**, on the system's fleet. The host that runs the
  queue starts it with the claim, the branch, the combination, the
  system and a token of sluice's app that reads the repository for an
  hour, and nothing else. The start role it assumes may change nothing
  the build runs. It clones the
  repository, starts the claimed run as a host would, and keeps its
  report. It keeps the checkout the checks ran in for [your own
  commands](#your-own-commands-after-the-checks), releases it after
  them, whatever they came to, and hands its report over as a zip. It
  holds no key and no secret.
- **The sign build**, which the rule starts when the run build ends,
  whatever it came to, unless someone stopped it. It runs none of the
  combination's code. It keeps the run build's report only where its
  SHA-256 is the one CodeBuild recorded as the run build uploaded it.
  It keeps only the results whose keys the combination's tree bears
  out, signs them with the key, and writes the run ended in the
  branch's state.

A run build someone stopped starts no sign build: the host that runs
the queue ends its run with no outcome, saying the build was stopped. So does a run build no
sign build took within the host's `[peers] alive`, naming the rule.

An instance runs one build at a time. A build releases its checkout,
and everything the run started in it, before it ends. A build CodeBuild
stopped may end before that, so each run build starts by ending every
container the instance runs and removing sluice's scratch and state, and
the file naming a checkout it kept, an earlier build left. Images stay, as the instance's caches do. The runs
an instance takes in turn share it, so a run's result claims no
confinement.

### Your own commands after the checks

Each run project's buildspec, in `.config/sluice-codebuild.yml`, leaves
room among its `post_build` commands for commands of your own after the
checks, such as pushing what the checks built to a shared cache. Write
them between its two `# sluice:` lines, at their indentation:

```yaml
                # sluice: your own commands, after the checks, in $SLUICE_CHECKOUT
                - |
                  if [ -n "${SLUICE_CHECKOUT:-}" ]; then
                    cd "$SLUICE_CHECKOUT"
                    ./scripts/push-cache
                  fi
                # sluice: end of your own commands
```

They run in the checkout the checks ran in, `cd "$SLUICE_CHECKOUT"`,
which still holds what the checks wrote. CodeBuild runs `post_build`
whatever the checks came to, so your commands run whether they passed
or failed; `$CODEBUILD_BUILD_SUCCEEDING` is `0` where a command before
them failed, a run that kept no report among them. The build releases the checkout in its `post_build` phase's
`finally` block, after your commands, whatever they came to. Then
deploy the stack again.

Your commands hold the run build's role, never sluice's keys. Give it a
managed policy of your own as `RunPolicies` when you deploy, allowing
writes to your store's cache and nothing else. The role's own policy
denies itself the signing key, the app's key, every CodeBuild action
and every IAM action, which no policy you give it can lift. GitHub
gives no token that writes one repository's packages alone outside
Actions, so a store on GitHub's registry is for the GitHub Actions
pool.

`sluice init codebuild` reads a template that differs only between
those lines as it writes it, and `sluice init codebuild --force` keeps
each run project's commands for the same system when it writes a newer
release's template, saying which it drops for a system it no longer
writes. A command you added after `sluice job run` before this room
existed would be written over: move it between the lines first. A line
between them that would reach past the `post_build` commands, such as
`artifacts:` at the buildspec's top level, is refused, by
`sluice init codebuild` with `--force` or not and by
`sluice pool check`, which names the run project and the line.

### What it costs

Instances bill while they run, each for an hour at least, whether or not
a run is on them. Size the fleet's base capacity from the first runs' times
and how long they waited, and its maximum from the branch's `parallel`.
The sign builds run on demand, for a minute or two each.

## With every host off

A pool takes runs; it doesn't run the queue. A round runs on the host
of yours that runs the branch's queue, so with every host off a
submission waits.
Rounds run as a job of their own are [designed, not built](/sluice/roadmap/).
