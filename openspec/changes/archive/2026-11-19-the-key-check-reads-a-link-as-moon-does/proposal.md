## Why

flume's `bolt/flume-is-tested-against-the-claude-code-hosts-run`,
submission 1, failed `root:format` on 2026-11-19 for nothing it changed:
`.claude/rules/core-details.md is not what the tree holds`. The file is a
link to `../../docs/architecture/core-details.md`. moon 2.6.1 hashes it
as it reads it on disk, by the content of the file it names. sluice's
key check read the tree with `git cat-file blob <revision>:<path>`,
which gives a link's own text, so it refused the key. Every submission
whose task inputs take in such a link failed. boomdrive-cloud holds nine
such links under `.claude/rules/`, flume one.

## What Changes

- **A link among a key's inputs is read as the file it names in the
  same tree.** The path is resolved from the link's own folder, as moon
  reads it on disk (the design agent's ruling of 2026-11-19; record
  0129).
- **A link the tree cannot follow to a file of its own stays refused.**
  One that leaves the tree, names a path the tree does not hold, loops
  or names a folder is no file of the tree, and a key naming it is
  refused as before: `<task>: <path> is not in the tree`.
- **Nothing else in the key check changes.**
- **Recorded.** Record 0129, and rule core.9's details.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pass-proof`: a key's input the tree holds as a link is read as the
  file it names in the same tree.

## Impact

- `crates/sluice/src/profiles/moon.rs`: `file_at` reads through
  `git cat-file --batch --follow-symlinks -Z`, taking a blob alone.
- Tests: `a_tracked_link_is_read_as_the_file_it_names` in
  `crates/sluice/src/profiles/moon.rs`, and
  `a_key_whose_input_is_a_link_is_proven` in
  `crates/sluice/tests/profiles/moon.rs`.
- Records and rules: record 0129; rule core.9.
- Sources: the C.2 moon row and C.6; the change `peers`, design D9;
  rules core.9 and core.22.

## Touches

```
crates/sluice/src/profiles/moon.rs
crates/sluice/tests/profiles/moon.rs
docs/adr/0129-a-keys-input-the-tree-holds-as-a-link-is-read-as-the-file-it-names.md
docs/architecture/core.md
openspec/changes/the-key-check-reads-a-link-as-moon-does/**
```
