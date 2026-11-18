---
number: 116
title: A branch is routed by its head's policy, and comes home when its signers go
status: accepted
date: 2026-11-12
decision-makers:
- agentplot-design
---

# A branch is routed by its head's policy, and comes home when its signers go

Status: accepted on 2026-11-12 by agentplot-design. It is the design agent's own choice, decided by the statement's 26 and C.6 and record 0037. It is cheap to change and holds no choice of the user's.

Its half on a branch coming home is carried out by record 0122's move between homes, and its own unit is not built.

Its routing half is replaced by record 0122, as built on 2026-11-14: a branch is routed by the home its records name in `home.json`, read by every process, and by the policy at its head only where no home holds its records anywhere.

Sources: the statement's C.6, as amended on agentplot/blueprints's branch `design/sluice-a-branch-comes-home-when-its-signers-go` (`38b00d5`); record 0037; `crates/sluice/src/profiles/branches.rs` `policy_of` and `routing`, `crates/sluice/src/profiles/records.rs` `route`, `lock` and `migrate`, at `v0.78.1` and `a9edcd90`; agentplot-ops's and wldn-ops's readings of switchboard-kit on chuck-herdr-alpha, 2026-11-12.

## Context and problem statement

switchboard-kit named a signer on main and `bolt/*` on 2026-11-10 and dropped it on 2026-11-11. Its bolts cut in between kept their queues on GitHub. When a take-main brought main's policy into `bolt/an-apps-repository-carries-no-switchboard-code`, sluice's processes on chuck-herdr-alpha stopped agreeing on where its queue was. A submit routed it to the host's own files and opened a try there. A round still read GitHub's state, which the landing had left with no lead and no candidates. A wait saw the submission once, then not at all.

Three things in the code meet there:

- A branch's route is read from the first revision of it the host holds, its own ref before the remote's tracking ref. Two refs of one branch can carry different policies.
- A process keeps a branch's route for its whole life, so a long-lived one keeps the route it read before the branch moved.
- A branch moves to the remote refs with its open entries (record 0037, `migrate`). Nothing moves one back when its policy stops naming signers.

## Considered options

1. **The head's policy routes the branch for every process, read again as it moves, and a branch whose signers go comes home with its open entries** (chosen).
2. **A branch on the remote stays there once moved.** A repository that drops its signers would keep a queue no trusted signer can run, and whose results no gate counts.
3. **Each process routes as now, from the first ref it holds.** Two processes on one host keep disagreeing about one branch.

## Decision outcome

- **One route, from the head.** A branch's records are routed by the policy at its head as its remote holds it, the revision its rounds read. Every process on the host reads that one answer, and reads it again whenever the branch moves. A branch with no remote is routed by its own ref, as now.
- **Home when the signers go.** The first round that reads a remote-held branch's policy naming no signers carries the remote state's open entries into the host's own files, each under a number of the host's. That is what record 0037 does the other way. It leaves the remote state where it is, unread. A run in flight on the remote has no outcome and runs again at home (60).
- **Said once.** That round says the branch came home, naming each entry carried and the number it now has, so a waiter on the old number learns the new one (26, record 0114).

### Consequences

- A take-main that brings a policy with no signers into a bolt brings its queue home in the same round. No submission is stranded, and none needs submitting again.
- A long-lived process and a new one read the same route for a branch.
- The statement's C.6 is amended on agentplot/blueprints's branch above, which the user pushes.
