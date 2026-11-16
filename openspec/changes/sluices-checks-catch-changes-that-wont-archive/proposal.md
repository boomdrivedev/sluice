## Why

`root:changes` fails an open change that `openspec archive` would refuse,
or that would archive into a spec `root:specs` then fails, such as one
that modifies a requirement past OpenSpec's 500 characters (record 0128;
core.61). Nothing in the gate shows it does. Record 0128 left the
refusals with no test of their own, and the requirement names no
scenario for a requirement past OpenSpec's length, the commonest case.
Flume's gate runs the same `check-changes.sh` over cases of its own in
`root:changes-cases`, one scratch tree for each scenario of its
requirement (Flume's record 0073, point 7; core.59). A change to the
check that let a long requirement through would pass sluice's gate today,
and be found only at an archive.

## What Changes

- **`root:changes-cases` holds `check-changes.sh` to its cases.**
  `check-changes-cases.sh`, copied from Flume's, runs the check over five
  scratch trees, each holding OpenSpec's configuration, a spec and the
  changes its case names, written as data, and checks the check's exit
  and its words. Each case is named for the scenario it answers.
- **The requirement names each case.** "Each open change is validated as
  it would archive" keeps its three scenarios and gains two, a
  requirement past OpenSpec's length and a change that fails
  `openspec validate --strict`, the two Flume's requirement has and
  sluice's lacks.
- **The decision is written down.** Record 0128 is amended: the cases are
  a task of their own, and its consequence that the refusals have no test
  goes. Rule core.61 names the cases as allowed.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `repository-gate`: `root:changes` names a scenario for each case, and
  `root:changes-cases` runs the check over them.

## Impact

- `moon.yml`, `check-changes-cases.sh` (new).
- Records and rules: record 0128 as amended; rule core.61.
- `check-changes.sh`, the open changes, sluice's records, its policy and
  the host's settings: nothing changes.
- Out of scope: Boomdrive's record 0050, the family's design for the
  check, which has no cases. Whether it takes them is Boomdrive's to
  decide.
