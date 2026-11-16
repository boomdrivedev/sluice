---
title: Roadmap
description: What is built, and what is designed and not yet built.
---

Each host runs its own queue until a policy names signers; then hosts
share one queue on the git remote. sluice lands its own `main` on one
host, gated on every test tier in a sandbox.

## Built

| | macOS | Linux |
|---|---|---|
| The queue, worktrees, checks, branch protection | ✓ | ✓ |
| Release binaries | Apple silicon | x86-64, ARM |
| Sandboxed checks | ✓ Seatbelt | planned: bubblewrap, Landlock |
| moon as the task runner, or one command | ✓ | ✓ |
| The host's shared caches, given to every run | ✓ | ✓ |
| An agent's own test runs, under the host's bound | ✓ | ✓ |
| Previews of what a branch landed, held until released | ✓ | ✓ |
| Telemetry over OTLP, opt-in | ✓ | ✓ |
| Many hosts sharing one queue on the git remote, with signed results: [executors and systems](/sluice/guides/executors-and-systems/) | ✓ | ✓ |
| A check required on several systems | ✓ | ✓ |
| Runs no host takes, on GitHub Actions, signed by the workflow itself: [pools](/sluice/guides/pools/#github-actions) | ✓ | ✓ |
| A queue's checks on a CodeBuild fleet of your own, signed by a key in AWS KMS: [pools](/sluice/guides/pools/#a-codebuild-fleet) | ✓ | ✓ |

## Designed, not built

- **Rounds run on GitHub Actions**: a job that runs a branch's rounds,
  so a submission lands with every host off.
- **Sandboxed checks on Linux**, with bubblewrap and Landlock, and memory
  and core limits by cgroup
  ([ADR 0012](/sluice/decisions/0012/)).
