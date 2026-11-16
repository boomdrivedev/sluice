---
number: 128
title: Each open change is validated as it would archive, alone against the tree's specs
status: proposed
date: 2026-11-15
decision-makers:
- the user, by boomdrivedev/blueprints principle 16 as amended on 2026-11-15
- the code stage of chore/sluices-changes-gate-validates-each-open-change-as-archived
- the code stage of chore/sluices-checks-catch-changes-that-wont-archive
---

# Each open change is validated as it would archive, alone against the tree's specs

Status: proposed. Point 1 is the user's, boomdrivedev/blueprints principle 16 as amended on 2026-11-15, which the chore's brief carries to this repository; the record waits on their yes. Points 2 to 7 are the code stage's own, made on the rules written and cheap to change. Points 2 to 5 are the choices Boomdrive's record 0050 made for its own tree.

Amended 2026-11-16, point 8, by the code stage of the chore `sluices-checks-catch-changes-that-wont-archive`, whose brief has sluice's checks fail a change OpenSpec would refuse to archive, such as one with a requirement over 500 characters, as Boomdrive's and Flume's checks do. The check did, but nothing in the gate showed it: the refusals had no test, and the requirement named no scenario for a requirement past OpenSpec's length. Flume's record 0073, point 7, runs its copy of the check over cases of its own. The point is the code stage's, made on the rules written (core.21) and cheap to change.

Deciders: the user. Their answer of 2026-11-15 on their card `req_278df7ab2cc34988a3f5`: "OpenSpec's own validation stays; the rule is about checks of what pages say". Principle 16 as amended keeps "OpenSpec's `openspec validate` of a kit's specs and its open changes, each change validated as it would archive". The brief of the chore `sluices-changes-gate-validates-each-open-change-as-archived`, on the bolt `bolt/sluices-checks-read-no-markdown`, which has `root:changes` fail where an open change would not archive as it is: each open change's deltas applied to the specs, and the result valid under `openspec validate --strict`, naming the change and what fails. The chore's code stage, on points 2 to 7. The code stage of the chore `sluices-checks-catch-changes-that-wont-archive`, on the bolt `bolt/sluices-changes-are-checked-as-archived`, on point 8.

Sources: boomdrivedev/blueprints `design/principles.md` 16 at `3a5c19d` on `design/a-tools-own-validation-stays`; Boomdrive's record 0050 and core.61, and its `check-changes.sh`, the pattern this repository copies; OpenSpec 1.14.3, the release `devenv.nix` pins, its `openspec archive` and `openspec validate`; rules core.21, core.24 and core.33; record 0125, decision 8, on `bolt/sluice-is-refactored-and-runs-on-pools`; for point 8, Flume's record 0073, point 7, its core.59 and its `check-changes-cases.sh` at `cdc3ddf3` on its main.

## Context and problem statement

`root:specs` runs `openspec validate --specs --strict`, and `root:changes` ran `openspec validate --changes --strict`. A change can pass that and still fail at its archive, or archive into specs that `root:specs` then fails:

- `openspec archive` refuses a delta that does not apply, such as an ADDED requirement its spec already holds with other text. `openspec validate --changes --strict` passes it.
- `openspec archive` checks the specs it rebuilds without `--strict`, so it writes a spec with a warning, which `root:specs` fails once the archive lands. A new capability whose delta has no `## Purpose` gets the placeholder Purpose archive writes. A MODIFIED requirement past OpenSpec's 500 characters is another, since `openspec validate --changes` holds only an ADDED one to that length.

Each is found only at the archive, after the gate has passed the branch. Four of the tree's eight open changes were such changes on 2026-11-15:

- `a-host-hands-over-only-to-the-host-its-mark-names`: two MODIFIED requirements of `request-intake-host` past 500 characters.
- `a-places-scratch-fits-a-socket`: one MODIFIED requirement of `place-scratch` past 500 characters.
- `actions-pool`: one MODIFIED requirement of `forge-identity` past 500 characters.
- `finished-submissions`: the new capability `finished-records`, with archive's placeholder Purpose.

## Considered options

For what a change is applied to:

1. **Each change alone, applied to a scratch copy of the tree's specs** (chosen).
2. Every open change applied in turn to one copy.
3. Each change applied to the tree's own specs, which the check then puts back.

For which specs are held to `--strict` after the archive:

1. **Each spec the archive writes, a new one or one that differs from the tree's** (chosen).
2. Every spec in the copy, as `openspec validate --specs --strict` reads them.

For the two changes built on the intake mark:

1. **Removed, never archived** (chosen).
2. Their long requirements cut to OpenSpec's length, and kept open.

For `actions-pool`'s MODIFIED requirement, `forge check verifies the setup`:

1. **The pool's reports an ADDED requirement of their own, and the tree's requirement left as it stands** (chosen).
2. The MODIFIED block written again on the tree's requirement, and split where it runs past 500 characters.

For how the check is shown to do what it says (point 8):

1. **Cases of its own, a task apart, each a scratch tree written as data** (chosen).
2. No test of its own, the gate's run of the check over the tree's own changes standing for one.
3. The cases inside `check-changes.sh`.

## Decision outcome

1. **`root:changes` holds each open change to OpenSpec's validation as it would archive (the user's).** It still runs `openspec validate --changes --strict`. Then, for each open change, it runs `openspec archive` and validates the specs that result with `--strict`. A change that fails is named with OpenSpec's words. `check-changes.sh` does it, and core.61 says so.
2. **Each change is applied alone to a scratch copy of the tree's specs (option 1).** The copy holds the tree's specs, `openspec/config.yaml` and that change. Each change archives at its own merge, onto its branch as it stands then.
   - Not taken: option 2. It judges a change by the other open changes and the order they are applied in, and neither is the change's own. Two open changes that conflict are found by the gate on the branch that lands second.
   - Not taken: option 3. A check writes nothing into the tree.
3. **The specs the archive writes pass `openspec validate --strict`, though `openspec archive` validates them without it.** `--strict` is how `root:specs` reads them once the change is archived.
4. **Only the specs the archive writes are validated (option 1).** A spec the change leaves as it was is `root:specs`' to check.
   - Not taken: option 2. A spec on the branch that fails would fail every open change, each named for what it did not write.
5. **A task left unchecked fails nothing.** The archive runs with `--yes`, and a change is open while its tasks are worked.
6. **The two changes built on the intake mark are removed, never archived (option 1).** `a-host-hands-over-only-to-the-host-its-mark-names` and `a-hand-overs-withdrawal-holds-no-request` say how a host keeps and hands over the intake mark. Rule core.33 has ruled the mark out since record 0093 as amended on 2026-11-08, so neither will archive as written. Record 0125's decision 8, on `bolt/sluice-is-refactored-and-runs-on-pools`, removes them too, and git merges a removal made on both sides with no conflict.
   - Not taken: option 2. It rewrites requirements for behaviour the rules rule out, and would conflict with each branch that removes the changes: the pools bolt, `bolt/sluice-runs-one-host-process` and `change/a-queues-home-is-written-in-its-records`.
7. **The other three open changes are made to archive with nothing refused and every spec they write valid, and stay open.** Whether each is built, archived or dropped is not this record's to decide.
   - `a-places-scratch-fits-a-socket`: its requirement on a worktree's former scratch is three, each with its own scenarios and its words kept: the move at the next run, a run beside a going run keeping the former scratch, and a former scratch the system cannot move being removed.
   - `actions-pool` (option 1): its MODIFIED block was written on 2026-10-25 against `forge check verifies the setup` as it stood then. Archived, it would put that wording back over the tree's, which has since said "every ruleset" and "With a key". The pool's reports, and the two scenarios on them, are an ADDED requirement, `forge check verifies a GitHub pool's setup`. Option 2 is not taken: the pool's sentences added to the tree's requirement run past 500 characters, and the block would go stale again with the next change to the requirement.
   - `finished-submissions`: its `finished-records` delta has a `## Purpose`, which archive carries into a new capability.
8. **`root:changes-cases` runs the check over cases of its own (option 1; amended 2026-11-16).** `check-changes-cases.sh`, copied from Flume's, runs `check-changes.sh` over a scratch tree for each scenario of repository-gate's "Each open change is validated as it would archive": a change that does not validate, one archive refuses, a new capability with no Purpose, a MODIFIED requirement past OpenSpec's 500 characters, and two changes applied alone with their tasks open. It checks the check's exit and its words, and that the tree is as it was. The changes are written into the scratch trees as data and only OpenSpec reads them, a tool's own validation of the files it works on (core.21), so no task reads a Markdown file of the tree. Each case is named for the scenario it answers. The requirement keeps its three scenarios, which a MODIFIED block must carry, and gains the two Flume's has.
   - Not taken: option 2. The tree's changes pass the check, so its run over them shows no refusal at all, and a change to the check that let a long requirement through would pass the gate and be found at an archive.
   - Not taken: option 3. The cases would run over every tree the check reads.
   - Boomdrive's record 0050, the family's design for the check, has no cases. Whether it takes Flume's is Boomdrive's to decide (CLAUDE.md, "The family").

### Consequences

- Good: a change that would not archive, or would archive into specs `root:specs` fails, fails its own branch's gate, named, before it lands.
- Good: a delta that adds a capability has to carry its own `## Purpose`.
- Bad: `root:changes` runs `openspec` once more for each open change, and once for each spec its archive writes, about seven seconds for seven changes on deneb.
- OpenSpec 1.14.3 reads an ADDED requirement that its spec holds word for word as already in sync, and archives it with no change. The check passes it, as archive does.
- Good: a change to `check-changes.sh` that lets through a change archive would refuse, or one that archives into a spec `root:specs` fails, fails `root:changes-cases`, naming the case (point 8).
- Bad: `root:changes-cases` runs `check-changes.sh` over five trees, each running `openspec` two to four times.
- Bad: sluice's `check-changes-cases.sh` and Flume's can drift, as the two `check-changes.sh` can.
- Rule core.61 holds the decision.
