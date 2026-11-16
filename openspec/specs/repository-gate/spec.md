# repository-gate Specification

## Purpose
What this repository's own gate checks of the tree a submission hands it,
so that every file the tree tracks is formatted, linted or excluded with a
reason, and every plugin is validated, type-checked and tested on every
submission.

## Requirements

### Requirement: Every tracked file is claimed or excluded

The format check SHALL fail when the tree tracks a file that no formatter
claims and no exclude names, and SHALL name that file.

#### Scenario: A file type nobody claims

- **WHEN** a submission adds `notes/plan.rst` and nothing claims `*.rst`
- **THEN** `moon run root:format` fails naming `notes/plan.rst`

#### Scenario: A file type an exclude names

- **WHEN** a submission adds a file under `docs/examples/`
- **THEN** `moon run root:format` passes without reading it

### Requirement: Every exclude says why

Every exclude in the formatter's configuration SHALL carry a comment
saying why no formatter owns those files.

#### Scenario: Reading the excludes

- **WHEN** a reviewer reads `treefmt.nix`
- **THEN** each exclude, or each group of excludes, has a comment above it
  giving its reason

### Requirement: TypeScript, JavaScript, CSS and JSON are formatted and linted

The format check SHALL fail on a `.ts`, `.tsx`, `.mjs`, `.css` or `.json`
file that Biome would rewrite or whose lint Biome's recommended rules
fail, outside the files excluded by name.

#### Scenario: An unformatted module

- **WHEN** a submission changes `plugins/sluice-board/hooks/board.ts` to
  use single quotes
- **THEN** `moon run root:format` fails naming the file

#### Scenario: A lint error

- **WHEN** a submission compares with `==` in a board module
- **THEN** `moon run root:format` fails naming the rule and the file

### Requirement: Python is formatted and linted

The format check SHALL fail on a `.py` file that ruff would reformat or
whose default rules ruff fails.

#### Scenario: An unformatted diagram script

- **WHEN** a submission changes `docs/diagrams/sluice.py` so ruff would
  reformat it
- **THEN** `moon run root:format` fails naming the file

### Requirement: Workflows are linted

The format check SHALL fail on a workflow under `.github/workflows/` that
actionlint rejects.

#### Scenario: A mistyped key

- **WHEN** a submission writes `runs-on` as `run-on` in `ci.yml`
- **THEN** `moon run root:format` fails naming the workflow and the line

### Requirement: The board is validated, type-checked and tested

The check-fast tasks `plugin-board:validate`, `plugin-board:typecheck` and
`plugin-board:test` SHALL fail when `plugins/sluice-board` fails, in turn,
`claude plugin validate --strict`, `tsc -p` against the engine types kept
beside it, or a test under `claude plugin test`.

#### Scenario: A type error

- **WHEN** a submission passes a number where `board.ts` declares a string
- **THEN** `moon run plugin-board:typecheck` fails naming the file and line

#### Scenario: A failing test

- **WHEN** a submission changes the band's wording and not its test
- **THEN** `moon run plugin-board:test` fails naming the test

### Requirement: The board's checks need no network and no home

The board's tasks SHALL pass inside the sandbox executor with the network
denied, writing nothing outside its place and its scratch directory, and
reading no Claude Code configuration or credentials of the user's.

#### Scenario: Under the gate

- **WHEN** sluice runs `#check-fast` on a submission under the sandbox
  executor
- **THEN** every `plugin-board` task passes, and the user's `~/.claude`
  and `~/.claude.json` are unchanged

### Requirement: The board's engine types match the pinned Claude Code

`plugin-board:engine-types` SHALL fail when the engine types kept beside
the board were written by a Claude Code release other than the one the
repository pins, naming both releases.

#### Scenario: The pin moves alone

- **WHEN** a submission moves the Claude Code pin and leaves the engine
  types as they were
- **THEN** `moon run plugin-board:engine-types` fails naming the types'
  release and the pinned release

### Requirement: The marketplace and every Claude Code plugin are validated

The check-fast task `root:marketplace` SHALL fail when the marketplace
fails `claude plugin validate --strict` or lists a plugin whose directory
has no manifest, and each Claude Code plugin's `validate` task SHALL fail
when its directory fails `claude plugin validate --strict`.

#### Scenario: A plugin missing from its directory

- **WHEN** a submission lists a plugin in `.claude-plugin/marketplace.json`
  whose directory has no manifest
- **THEN** `moon run root:marketplace` fails naming the plugin

#### Scenario: A broken plugin manifest

- **WHEN** a submission removes the `name` from
  `plugins/sluice/.claude-plugin/plugin.json`
- **THEN** `moon run plugin-sluice:validate` fails

### Requirement: The board's type configuration is tracked

`plugins/sluice-board` SHALL hold a tracked `tsconfig.json`, so loading
the board in a session leaves the worktree clean.

#### Scenario: After a session loads the board

- **WHEN** a session started with `--plugin-dir plugins/sluice-board`
  loads the board in a worktree
- **THEN** `git status --porcelain` in that worktree prints nothing

### Requirement: Every plugin, and the site, is a project of its own

Each directory under `plugins/`, and `site/`, SHALL be a moon project
listed in `.moon/workspace.yml` with its language declared, and each of
its checks SHALL be a check-fast task of that project, not of the root or
another part's. The site's one check SHALL be its prose check,
`site:prose`: it has no tests, and its build is the Pages workflow's.

#### Scenario: Listing the projects

- **WHEN** `moon query projects` runs in the repository
- **THEN** it lists `plugin-sluice`, `plugin-board`, `plugin-herdr` and
  `site` with the languages `bash`, `typescript`, `bash` and `typescript`

#### Scenario: Listing the root's tasks

- **WHEN** `moon query tasks` runs in the repository
- **THEN** no task of `root` runs a script under a plugin's directory

### Requirement: The flake is a project of its own

The flake's `nix/` SHALL be a moon project, `nix`, listed in
`.moon/workspace.yml` with the language `nix`. Its check-fast tasks SHALL
be `nix:lint`, statix over `flake.nix` and `nix/`; `nix:typecheck`,
every system's package and each module in a system of its platform; and
`nix:test`, the cases under `nix/tests` and the activation's. Its format
SHALL be `root:format`'s (rule core.21; record 0098).

#### Scenario: Listing the flake's project

- **WHEN** `moon query projects` runs in the repository
- **THEN** it lists `nix` with the language `nix`, and
  `moon query tasks` lists `nix:lint`, `nix:typecheck` and `nix:test`,
  each tagged `check-fast`

### Requirement: The flake's checks need no store and no network

`nix:typecheck` and `nix:test` SHALL evaluate offline and read-only
against a store that is none of the host's, reading the flake's inputs
where the devenv shell fetched them, by the nix `devenv.nix` pins, as
`nix:lint` runs the statix it pins. Each failure SHALL be one line
naming its case, and every case SHALL run (rules core.21, core.24).

#### Scenario: A failing case

- **WHEN** a case under `nix/tests` fails and `moon run nix:test` runs
- **THEN** the task fails with one line, `nix: <case>: <what>`, for each
  failure, and every other case still runs

#### Scenario: No store and no network

- **WHEN** `sluice exec <base> -- moon run nix:test` runs under the
  gate's sandbox, which allows only localhost
- **THEN** `nix eval` reads the flake's inputs from the paths the devenv
  shell names, writes nothing to any store, and the task passes

### Requirement: The module's rendered files are kept byte for byte

The files under `nix/tests/files/`, which the flake's cases compare with
the module's text byte for byte, SHALL be excluded from the formatter by
name, with a comment saying why.

#### Scenario: Formatting the tree

- **WHEN** `moon run root:format` runs on a tree holding
  `nix/tests/files/every.toml`
- **THEN** it does not read that file, and the exclude in `treefmt.nix`
  has a comment above it giving its reason

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
