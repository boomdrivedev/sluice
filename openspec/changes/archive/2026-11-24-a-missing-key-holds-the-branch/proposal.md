## Why

Boom's accepted decision `a-key-the-tree-does-not-bear-out-holds-the-branch`.
From 2026-11-16 to 2026-11-23, 26 combinations in 9 repositories failed
`root:format` on a key that named a file the tree did not hold:
`.claude/settings.json`, `.devenv/bootstrap/...`, `.pre-commit-config.yaml`.
No submission caused it, and no submission could fix it. sluice blamed
17 submissions for it, and their agents' loops took 31% of the hours from
a work's first submission to its landing that week.

## What Changes

- A check whose key names a file the combination's tree does not hold,
  absent from the tree or held there with other content, blames no
  submission. Nothing is recorded under its key. The check has no
  outcome, and the run is not run again for it.
- The branch holds on it, with the check and the file named, in
  `branch.json` as `key_held` and in the log as one `key_held` line.
  The entries of each combination refused so stay queued, out of the
  chain.
- An entry that was not in such a combination is formed and tested as
  any is, so work that changes the check's inputs can land. An entry
  refused alike joins the hold.
- The hold ends, logged once as `key_hold_ended`, at the first revision
  of the branch that changes the check's inputs, by the check's affected
  reading, or once no entry it holds is left. Its entries are then
  formed again on the branch.
- `sluice status <branch>` says the hold with the check, the file, the
  entries it holds and the way on, and `--json` gives it as `key_held`.
- A key refused for any other cause, a manifest that does not hash to
  its key among them, is blamed with why, as before.

## Capabilities

### New Capabilities

- `key-hold`: a branch held on a check whose key names a file the tree
  does not hold.

### Modified Capabilities

- `pass-proof`: a key the tree does not bear out is blamed only where it
  names no file the tree does not hold.

## Impact

- `crates/sluice-seams`: `Refused` names the file.
- `crates/sluice/src/profiles/moon.rs` and `checks.rs`: `check_key`
  types the input the tree does not hold as `NotHeld`, and `proven`
  passes its file on.
- `crates/sluice-core`: the hold in `branch.json`, its log lines, the
  round's chain and the run's verdict; `Status` carries it.
- `crates/sluice`: the status's line and JSON.
- `crates/sluice-fakes`: `lacks`, a task whose key names a file.

Out of scope: a run of a pool's job, whose sign job leaves such a key
unsigned, which reads as missing as before; and a notice to the
operator beyond the branch's status, which the decision leaves open.
