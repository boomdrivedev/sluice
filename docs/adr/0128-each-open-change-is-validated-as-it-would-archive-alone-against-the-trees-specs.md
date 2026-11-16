---
number: 128
title: Each open change is validated as it would archive, alone against the tree's specs
status: proposed
date: 2026-11-15
decision-makers:
- the user, by boomdrivedev/blueprints principle 16 as amended on 2026-11-15
- the code stage of chore/sluices-changes-gate-validates-each-open-change-as-archived
---

# Each open change is validated as it would archive, alone against the tree's specs

Status: proposed. Point 1 is the user's, boomdrivedev/blueprints principle 16 as amended on 2026-11-15, which the chore's brief carries to this repository; the record waits on their yes. Points 2 to 7 are the code stage's own, made on the rules written and cheap to change. Points 2 to 5 are the choices Boomdrive's record 0050 made for its own tree.

Deciders: the user. Their answer of 2026-11-15 on their card `req_278df7ab2cc34988a3f5`: "OpenSpec's own validation stays; the rule is about checks of what pages say". Principle 16 as amended keeps "OpenSpec's `openspec validate` of a kit's specs and its open changes, each change validated as it would archive". The brief of the chore `sluices-changes-gate-validates-each-open-change-as-archived`, on the bolt `bolt/sluices-checks-read-no-markdown`, which has `root:changes` fail where an open change would not archive as it is: each open change's deltas applied to the specs, and the result valid under `openspec validate --strict`, naming the change and what fails. The chore's code stage, on points 2 to 7.

Sources: boomdrivedev/blueprints `design/principles.md` 16 at `3a5c19d` on `design/a-tools-own-validation-stays`; Boomdrive's record 0050 and core.61, and its `check-changes.sh`, the pattern this repository copies; OpenSpec 1.14.3, the release `devenv.nix` pins, its `openspec archive` and `openspec validate`; rules core.21, core.24 and core.33; record 0125, decision 8, on `bolt/sluice-is-refactored-and-runs-on-pools`.

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

### Consequences

- Good: a change that would not archive, or would archive into specs `root:specs` fails, fails its own branch's gate, named, before it lands.
- Good: a delta that adds a capability has to carry its own `## Purpose`.
- Bad: `root:changes` runs `openspec` once more for each open change, and once for each spec its archive writes, about seven seconds for seven changes on deneb.
- OpenSpec 1.14.3 reads an ADDED requirement that its spec holds word for word as already in sync, and archives it with no change. The check passes it, as archive does.
- The refusals have no test of their own, as Boomdrive's have none: a test would hold specs as fixtures, which principle 16 keeps no task reading, and the gate runs the task over the tree's own changes. They were seen once by hand, on a scratch copy holding a change that adds a requirement its spec holds with other text, and one that adds a capability with no Purpose.
- Rule core.61 holds the decision.
