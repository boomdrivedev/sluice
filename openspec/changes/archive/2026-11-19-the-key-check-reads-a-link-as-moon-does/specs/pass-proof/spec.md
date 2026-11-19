## ADDED Requirements

### Requirement: A key's input the tree holds as a link is read as the file it names
Where a key's manifest names an input the combination's tree holds as a
link, the check profile SHALL check it against the file the link names
in the same tree, resolved from the link's own folder, as moon reads it
on disk. A link that leaves the tree, or names a path the tree does not
hold, SHALL be read as no file of the tree, and its key refused (C.2
moon row, C.6; record 0129).

#### Scenario: A link to a file in the tree
- **WHEN** a moon run passes `alpha:test`, whose input `alpha/notes.md` the tree holds as a link to `../docs/notes.md`
- **THEN** moon's manifest gives `alpha/notes.md` the sha256 of `docs/notes.md`, and `alpha:test` is proven
  (`a_key_whose_input_is_a_link_is_proven` in
  `crates/sluice/tests/profiles/moon.rs`)

#### Scenario: A link that leaves the tree or names nothing
- **WHEN** a key's manifest names an input the tree holds as a link to `/etc/hosts`, to `../../../../outside` or to a path the tree does not hold
- **THEN** the key is refused with `root:format: <path> is not in the tree`, while one naming `.claude/rules/core.md`, a link to `../../docs/arch/core.md`, is borne out
  (`a_tracked_link_is_read_as_the_file_it_names` in
  `crates/sluice/src/profiles/moon.rs`)
