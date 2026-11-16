## ADDED Requirements

### Requirement: Each open change is validated as it would archive

`root:changes` SHALL fail where an open change under `openspec/changes`
fails `openspec validate --strict`, where `openspec archive` refuses it,
applied alone to a copy of the tree's specs, or where a spec that
archive writes fails `openspec validate --strict`. It SHALL name the
change with OpenSpec's words, SHALL write nothing in the tree, and SHALL
fail nothing for a task left unchecked (principle 16 as amended
2026-11-15; record 0128; rule core.61).

#### Scenario: A change that adds a requirement its spec holds

- **WHEN** an open change adds a requirement under a name its spec
  already holds, with other text, and `moon run root:changes` runs
- **THEN** the task fails naming the change, with archive's words that
  the requirement already exists, and the tree's specs are as they were

#### Scenario: A change that archives into a spec root:specs would fail

- **WHEN** an open change adds a capability whose delta has no
  `## Purpose`, and `moon run root:changes` runs
- **THEN** the task fails naming the change and the spec, with
  OpenSpec's words that its Purpose is a placeholder

#### Scenario: A change whose tasks are open

- **WHEN** every open change archives into valid specs, one with tasks left
  unchecked, and `moon run root:changes` runs
- **THEN** the task passes
