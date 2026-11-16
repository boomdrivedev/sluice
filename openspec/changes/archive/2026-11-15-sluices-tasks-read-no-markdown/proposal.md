## Why

Principle 16 of boomdrivedev/blueprints, as amended 2026-11-15
(3a5c19d), says no check, test or gate task reads a Markdown file: what
a page says is kept by the people and agents who write and review it,
never by code that greps it, and a tool's own validation of the files it
works on stays. Rule core.21 has held it since 2026-11-11, and four of
sluice's tasks still read its Markdown: `root:adrs` greps the records'
frontmatter for their numbers, `root:openspec-claude` compares the
skills and commands OpenSpec writes under `.claude`, `site:prose` runs
Vale over the site's pages, and `site:rules` compares the committed
rules page with `docs/architecture/core.md`. Record 0127 decides how
they go.

## What Changes

- **Four tasks go.** `root:adrs`, `root:openspec-claude`, `site:prose`
  and `site:rules` leave the gate, with `check-adrs.sh`.
- **The rules page is made at build.** `site/scripts/rules.mjs` writes
  the site's rules page from `docs/architecture/core.md` before every
  build and dev server, as `adrs.mjs` writes the decision pages, and
  git no longer tracks it.
- **OpenSpec's writer stays.** `check-openspec-claude.sh` becomes
  `write-openspec-claude.sh`, which writes OpenSpec's Claude Code
  skills and commands with the pinned release when the pin moves, and
  compares nothing.
- **What stays.** `root:specs` and `root:changes`, OpenSpec's own
  validation; `root:toolchain`; `root:format`, whose treefmt reads no
  Markdown; and each plugin's `validate`, Claude Code's own validation
  of a plugin.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `repository-gate`: the site holds no check, and no task reads a
  Markdown file but by a tool's own validation of it.
- `refused-writes`: what the pages say of refused writes is kept by
  their writers, with no prose check named.

## Impact

- `moon.yml`, `site/moon.yml`, `site/package.json`,
  `site/scripts/rules.mjs`, `.gitignore`, `check-adrs.sh` (removed),
  `write-openspec-claude.sh` (from `check-openspec-claude.sh`),
  `devenv.nix`, `treefmt.nix`, `.vale.ini`, `site/README.md`;
  `site/src/content/docs/architecture/rules.md` leaves the tree.
- Records and rules: record 0127; record 0101's account of OpenSpec's
  files, amended; rule core.21's details.
- Records, policy and the host's settings of sluice's: nothing changes.
- Out of scope: Vale itself. `devenv shell -- prose` and the Claude
  Code hook stay as aids a writer runs or reads, which hold no gate.
