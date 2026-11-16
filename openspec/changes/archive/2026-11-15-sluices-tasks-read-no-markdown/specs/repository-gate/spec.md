## MODIFIED Requirements

### Requirement: Every plugin, and the site, is a project of its own

Each directory under `plugins/`, and `site/`, SHALL be a moon project
listed in `.moon/workspace.yml` with its language declared, and each of
its checks SHALL be a check-fast task of that project, not of the root or
another part's. The site SHALL hold no check: its pages are Markdown,
which no task reads (principle 16; core.21), it has no tests, and its
build is the Pages workflow's.

#### Scenario: Listing the projects

- **WHEN** `moon query projects` runs in the repository
- **THEN** it lists `plugin-sluice`, `plugin-board`, `plugin-herdr` and
  `site` with the languages `bash`, `typescript`, `bash` and `typescript`

#### Scenario: Listing the root's tasks

- **WHEN** `moon query tasks` runs in the repository
- **THEN** no task of `root` runs a script under a plugin's directory

## ADDED Requirements

### Requirement: No task reads a Markdown file

No task of the repository's SHALL read a Markdown file, a record, a
spec, a skill, a command, a README or a site page, to format, lint, test
or check it (principle 16 as amended 2026-11-15, boomdrivedev/blueprints
3a5c19d; core.21; record 0127). A tool's own validation of the files it
works on stays: OpenSpec's `openspec validate` of the specs and the open
changes, and Claude Code's `plugin validate` of each plugin.

#### Scenario: Listing the tasks

- **WHEN** `moon query tasks` runs in the repository
- **THEN** it lists no `root:adrs`, `root:openspec-claude`,
  `site:prose` or `site:rules`, and lists `root:specs`, `root:changes`,
  `root:toolchain` and `root:format`

### Requirement: The site's rules page is made at build

The site's build SHALL make its rules page from
`docs/architecture/core.md`, as it makes its decision pages from
`docs/adr`, and the tree SHALL NOT track the page (record 0127).

#### Scenario: Building the site

- **WHEN** the site's build runs, or its dev server starts
- **THEN** `site/scripts/rules.mjs` writes
  `site/src/content/docs/architecture/rules.md` from
  `docs/architecture/core.md` before Astro reads the pages, and
  `git ls-files site/src/content/docs/architecture/rules.md` prints
  nothing
