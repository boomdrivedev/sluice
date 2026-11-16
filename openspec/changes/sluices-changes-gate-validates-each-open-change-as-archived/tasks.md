# Tasks

Work in the place on
`chore/sluices-changes-gate-validates-each-open-change-as-archived`, cut
from `bolt/sluices-checks-read-no-markdown`, the branch it lands on and
so the base every `sluice exec` below names. Commit by pathspec, with
Conventional Commits scoped to the part of the tree, citing clauses by
number and rules by id.

## 1. The open changes archive into valid specs

- [x] 1.1 Remove `a-host-hands-over-only-to-the-host-its-mark-names` and `a-hand-overs-withdrawal-holds-no-request`. In `a-places-scratch-fits-a-socket`'s `place-scratch` delta, make the requirement on a worktree's former scratch three, each with its scenarios; in `actions-pool`'s `forge-identity` delta, make the pool's reports from `sluice forge check` an added requirement of their own; give `finished-submissions`' `finished-records` delta a Purpose. True when `sluice exec bolt/sluices-checks-read-no-markdown -- moon run root:changes` passes once 2.1 is done.

## 2. root:changes validates each open change as archived

- [x] 2.1 Add `check-changes.sh`, copied from Boomdrive's, and run it in `root:changes`, with the script among the task's inputs. True when `sluice exec bolt/sluices-checks-read-no-markdown -- moon run root:changes` passes. The two refusals have no test of their own, as Boomdrive's have none: a test would hold specs as fixtures, and the gate runs the task over the tree's own changes.

## 3. The decision is written down

- [x] 3.1 Add record 0128 (`docs/adr/0128-each-open-change-is-validated-as-it-would-archive-alone-against-the-trees-specs.md`) with the paths not taken, and rule core.61 with its details. True when `sluice exec bolt/sluices-checks-read-no-markdown` passes.

## 4. The specs archive clean

- [ ] 4.1 Archive the change into `openspec/specs/repository-gate/spec.md`. True when `sluice exec bolt/sluices-checks-read-no-markdown -- moon run root:changes` passes before the archive, and `sluice exec bolt/sluices-checks-read-no-markdown -- moon run root:specs` after it.
