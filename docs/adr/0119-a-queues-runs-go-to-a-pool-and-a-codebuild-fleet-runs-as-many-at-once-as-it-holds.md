---
number: 119
title: A queue's runs go to a pool, and a CodeBuild fleet runs as many at once as it holds
status: accepted
date: 2026-11-12
decision-makers:
- the user
- agentplot-design
---

# A queue's runs go to a pool, and a CodeBuild fleet runs as many at once as it holds

Status: accepted by the user on 2026-11-12, in agentplot-design's pane.

Sources: switchboard-kit's `.config/sluice.toml` on its main (agentplot-wldn/switchboard-kit); sluice's guide *Parallel and batch*; the statement in agentplot/blueprints, `design/sluice/requirements.md`, clauses 71, 77, 78, 79, 88, 107, 109, 141, C.3's launched executor, C.7 and C.8; the change `actions-pool` on main; record 0016; record 0122; AWS's CodeBuild guide, *Run builds on reserved capacity fleets* and *Reserved capacity fleet properties* (docs.aws.amazon.com/codebuild/latest/userguide/fleets.html, fleets.reserved-capacity-fleets.html), and CodeBuild's pricing page.

## Context and problem statement

Switchboard's queue runs every check on the EC2 box that holds it. Its main gate ends in Docker end-to-end tests that take 20 to 40 minutes a run. Its main tests one combination at a time (`batch = true`, at most three submissions to a batch), its bolts take the box's other runs, and the box's bound holds all of them together to about four. Submissions have waited days for their turn.

sluice's statement already has pools: a job service that starts a machine for each run the lead gives it, after the standing executors (88, 107). It names one pool, GitHub Actions, whose change `actions-pool` is written and not built. No pool is built, and the guides document none.

## Considered options

1. **Each run is a build on a CodeBuild reserved-capacity fleet, a pool beside GitHub Actions** (chosen).
2. **Standing executors on EC2 instances in an auto scaling group, each claiming runs.** Each would be a host sluice keeps running, with its own bound, leases and caches, and scaling them would fall to sluice or to ops. CodeBuild scales a fleet, keeps its instances warm and starts one build per run.
3. **A larger box.** It runs more at once on one machine, with every stack sharing one Docker engine and one memory, beside the box's sessions. It grows to a size and stops there.
4. **CodeBuild on demand, with no fleet.** Every build starts on a cold machine that installs the toolchain and pulls every image again, which a 20-minute gate pays on every run.

## Decision outcome

### The user's

- **Every run of the queue goes to the pool.** No standing machine has room for a queue whose gate runs Docker end-to-end tests, so a branch on a pool runs all of its checks there.
- **The pool sets how many run at once, and it holds about ten.** A queue's capacity is the number of machines running its combinations, not one box's bound of about four. A single run still takes as long as its gate, 20 to 40 minutes.

### agentplot-design's

- **CodeBuild is a pool profile.** The policy names a CodeBuild project and its fleet. The lead starts one build per run, the build's start is its claim and its end its report, and a build CodeBuild loses or stops is a lapsed claim (77, 88). Which instance takes a build is CodeBuild's.
- **A gate that trusts only a pool's signer runs every check on the pool.** No standing executor signs as a signer that gate trusts, so none can take its runs (107 as written). A pool's runs take no part of a host's bound (71). How many run at once is the branch's own `parallel`, which the guide tells a branch on a fleet to set to the fleet's maximum.
- **A branch on a fleet tests its combinations side by side.** Under `batch = true` a branch tests one combination at a time, so ten machines would run one of main's runs at a time. The guide tells a branch on a fleet to test with `batch = false` and `parallel` at the fleet's maximum. Each submission then lands about one run after it was submitted while the runs pass. Switchboard's policy is Switchboard's to change.
- **No candidate's code and no session can reach the signing key.** A build that runs a candidate's checks holds no key. A step that runs none of the candidate's code signs only the keys the candidate's tree bears out, as the GitHub Actions pool's run and sign jobs do (`actions-pool`). It signs with a key in AWS KMS that only that step's role may use. The policy lists the key's public half, `[signers.<name>] kms`, beside `ssh` and `github`, so every host verifies a run from the records alone and calls no AWS service (78, 141). The lead starts builds as an identity no session can use, and that identity cannot change what a build runs: no buildspec, image or environment of its own (C.8).
- **A warm instance starts the checks at once.** The fleet keeps a base of instances running. Each keeps, between builds, the Docker image layers, the toolchain's shell and the task runner's cache, so a build on a warm instance installs nothing and pulls no image it already holds. The fleet scales out to its maximum when builds wait. Builds past the maximum queue for an instance, since on-demand machines start cold and the branch's `parallel` already holds its runs to the fleet's size.
- **A build leaves its instance clean.** It runs the queue's release before it ends, whatever its outcome (record 0016), since the fleet's next build runs on the same instance. A run that reaches a container engine on a reused machine is never confined (79, C.8), as the box's open runs are not today.
- **A pool's run is a run like any other.** Its job's start is the run's claim and its end the run's report, in the states record 0122 writes, and its records reach the branch through its signer's ref (109).
- **A fleet is sized from measurements.** Each instance has the memory one integration stack needs, as the box's runs show it. The base and the maximum come from the trial's run times and the queue's measured waiting (principle 15). A fleet bills by the instance-minute, from each instance's start, with an hour's minimum.

### Consequences

- The queue's capacity grows with the fleet, not with the box, and the box's sessions keep their machine.
- A failed run on a branch tested side by side throws away the combinations stacked on it, which costs fleet minutes and no one's wait beyond that run.
- A warm instance's caches serve every build the fleet runs, and every project in the account that shares the fleet. The task runner's cache decides no verdict (108), and image layers are fetched by digest.

## Amendment, 2026-11-16: what the build of the GitHub Actions pool chose

The change `a-runs-life-is-written-first-and-actions-runs-its-checks` builds the GitHub Actions pool, the change `actions-pool`'s design, with record 0122's run life. Its choices are the build's own, cheap to change, and hold no choice of the user's; the CodeBuild fleet above is not built by it.

- **One workflow run, two jobs.** `sluice.yml`, which `sluice init actions` writes with the release that writes it, runs on a dispatch titled `sluice <claim>`. Its run job, on the runner the pool names for the run's system, reads the repository alone, with no environment, identity token or secret, and runs `sluice job run`. Its sign job follows whatever the run job came to, in the `sluice` environment, with the identity token and attestations, runs none of the combination's code, and runs `sluice job sign`, `actions/attest` pinned by commit, and `sluice job record`. *Not taken:* one job holding `id-token: write`, in which any check could ask for the token and sign any key.
- **`sluice job` is three commands.** `run` starts the run its claim holds through `start_claimed`, reading the branch's records through a state that holds its writes and sends them nowhere, its room held in its own process, and hands its report and the manifests its keys' walk reads to the sign job as an artifact. `sign` reads the run from the branch's records, never the run job's word, fetches the combination by its own ref, and keeps only the keys its tree bears out by those manifests and the runner the policy names. `record` adds the bundle among the pool's signer's runs, then writes the run ended from the bundle's results, a replay unproven, and closed in one write, refusing with nothing written where the claim no longer holds the run; it writes as the app the `sluice` environment names. A run the lead wrote running from its run job's start is still the job's to start.
- **The pool's seam.** `Tests::job` answers none, queued, running since the run job's start, or ended, recorded or not; the lead claims before it dispatches, dispatches only where no workflow run bears the claim, writes `running` from the run job's start, and reads a run ended with nothing recorded as a lapsed claim.
- **Keyless signatures verified against the roots sluice carries.** A `github` signer names its workflow and ref; its records are Sigstore bundles verified with `sigstore-verify` against the public-good root and GitHub's, carried in the build, by the certificate's identity in the repository the branch's remote names, with nothing fetched. A statement is held to a run statement's checks, and a bundle no carried root verifies counts for nothing, naming the identity it carried. *Not taken:* roots fetched by TUF and cached on the host, which calls a service at a read.
- **A pull request's head runs count by key.** `sluice-pr.yml` runs a request's head on each of the pool's systems with no secret and no identity; `sluice.yml`'s `sign-head` job signs each system's run as the sign job signs a claimed one, and `sluice job record --head` adds it under no claim, writing no state. A combination reads those results by key, so a check whose key passed on the head runs nothing again. A `pull_requests.checks` entry naming sluice's own check is refused. *Consequence:* a head's run job runs the request's own `sluice-pr.yml`, so the outcomes its report gives are the request's word, as a combination's checks' are on a host: the sign job proves the keys, not the outcomes.
- **Setting up and checking it.** `sluice init actions` writes both workflows, leaving one that differs unless `--force`, and appends `[signers.forge]` and `[pools.forge]` as text where the policy names no pool on GitHub. `sluice forge check` reads the workflow on the signer's branch as init writes it, the app's permission to run workflows, the `sluice` environment deployable from that branch alone, and the signer naming the pool's workflow.
