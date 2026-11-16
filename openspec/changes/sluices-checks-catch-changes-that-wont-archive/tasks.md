# Tasks

Work in the place on
`chore/sluices-checks-catch-changes-that-wont-archive`, cut from
`bolt/sluices-changes-are-checked-as-archived`, the branch it lands on
and so the base every `sluice exec` below names. Commit by pathspec, with
Conventional Commits scoped to the part of the tree, citing clauses by
number and rules by id.

## 1. The check is held to its cases

- [x] 1.1 Add `check-changes-cases.sh`, Flume's copied whole but for its header, and run it in `root:changes-cases`, tagged `check-fast`, with both scripts among the task's inputs. True when `sluice exec bolt/sluices-changes-are-checked-as-archived -- moon run root:changes-cases` passes, and fails naming "A modified requirement past OpenSpec's length" over a `check-changes.sh` whose spec validation is made to pass.

## 2. The decision is written down

- [ ] 2.1 Amend record 0128 with the cases as a task of their own, and rule core.61's details with the cases' scratch trees as allowed. True when `sluice exec bolt/sluices-changes-are-checked-as-archived` passes.

## 3. The specs archive clean

- [ ] 3.1 Archive the change into `openspec/specs/repository-gate/spec.md`. True when `sluice exec bolt/sluices-changes-are-checked-as-archived -- moon run root:changes` passes before the archive, and `sluice exec bolt/sluices-changes-are-checked-as-archived -- moon run root:specs` after it.
