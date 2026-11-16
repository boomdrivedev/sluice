# Tasks

Work in the place on `chore/sluices-tasks-read-no-markdown`, cut from
`bolt/sluices-checks-read-no-markdown`, the branch it lands on and so the
base every `sluice exec` below names. Commit by pathspec, with
Conventional Commits scoped to the part of the tree, citing clauses by
number and rules by id.

## 1. The four tasks go

- [x] 1.1 Remove `adrs` and `openspec-claude` from `moon.yml`, and `check-adrs.sh` from the tree. Rename `check-openspec-claude.sh` to `write-openspec-claude.sh`, which writes OpenSpec's Claude Code skills and commands with the release `devenv.nix` pins and compares nothing, and name it in `devenv.nix`'s note on the pin. True when `moon query tasks` lists neither, and `sluice exec bolt/sluices-checks-read-no-markdown` passes.
- [x] 1.2 Remove `prose` and `rules` from `site/moon.yml`. Have `site/scripts/rules.mjs` write the rules page with no `--check`, run it in `site/package.json`'s `prebuild` and `predev` beside `adrs.mjs`, add `/docs/architecture/core.md` to `site:build`'s inputs, ignore the page in `.gitignore` and remove it from the tree; say so in `site/README.md`, `.vale.ini` and `treefmt.nix`. True when `moon query tasks` lists `site:build` alone of the site's, `bun run build` in `site/` writes the rules page, and `sluice exec bolt/sluices-checks-read-no-markdown` passes.

## 2. The decision is written down

- [x] 2.1 Add record 0127 (`docs/adr/0127-sluices-tasks-read-no-markdown.md`) with the paths not taken, amend record 0101's account of OpenSpec's files, and add record 0127 and principle 16 as amended 2026-11-15 to rule core.21's details, with what a tool's own validation leaves in the gate. True when `sluice exec bolt/sluices-checks-read-no-markdown` passes.

## 3. The specs archive clean

- [x] 3.1 Validate the change, and archive it into `openspec/specs/repository-gate/spec.md` and `openspec/specs/refused-writes/spec.md`. True when, under the openspec `devenv.nix` pins, `openspec validate sluices-tasks-read-no-markdown --strict --no-interactive` passes before the archive, and `sluice exec bolt/sluices-checks-read-no-markdown -- moon run root:specs` passes after it.
