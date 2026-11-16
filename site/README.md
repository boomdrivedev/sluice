# The documentation site

The developer documentation at https://agentplot.github.io/sluice, an
[Astro](https://astro.build) site with [Starlight](https://starlight.astro.build),
published by `.github/workflows/pages.yml` on every push to `main` that
touches `site/`, `docs/` or the README.

```sh
cd site
bun install            # the pinned dependencies, from bun.lock
bun run dev            # http://localhost:4321/sluice/
bun run build          # dist/, with every internal link checked
```

A writer can run [Vale](https://vale.sh) over the prose from the
repository root, with the rules in `styles/Sluice` and the settings in
`.vale.ini`: `devenv shell -- prose`. No task of the gate reads the pages
(record 0127). The generated `decisions/` pages are left out, since a
decision record says what was weighed, and so is the rules page, whose
rules are `docs/architecture/core.md`'s own words.

## Where the pages live

`src/content/docs/`, one Markdown file per page, with the sidebar built
from the directories in `astro.config.mjs`:

| directory | pages |
|---|---|
| `index.mdx` | the start page: why, the four steps, install, quick start |
| `guides/` | the walkthrough |
| `reference/` | configuration, commands, state, telemetry |
| `architecture/` | how sluice is built; `rules.md` is **generated** from the rule list of `docs/architecture/core.md` by `scripts/rules.mjs` before every build and dev; not committed |
| `decisions/` | **generated**: one page per record in `docs/adr/`, by `scripts/adrs.mjs` before every build and dev; not committed |
| `many-hosts/` | the multi-host design, marked as under review and not built |
| `roadmap.md` | what is built and what is designed |

The brand marks are copied from `docs/brand/` into `src/assets/` and
`public/favicon.svg`, and the social card into `public/og.png`.

## The look and the start page

| file | what |
|---|---|
| `src/styles/brand.css` | the brand's colors and type over Starlight's, in both themes |
| `src/components/Hero.astro` | the start page's hero, with the animated diagram; other pages keep Starlight's |
| `src/components/Diagram.astro` | the diagram, inlined from `src/generated/sluice.svg` |
| `src/components/Flow.astro`, `Step.astro` | the start page's four steps; their prose is in `index.mdx` |

`src/generated/sluice.svg` is **generated**, by `scripts/diagram.mjs`
running `docs/diagrams/sluice.py --scoped` before every build and dev, and
is not committed. Change the diagram in `sluice.py`, then regenerate
`docs/diagrams/sluice.svg` for the README.

## Conventions

The pages use the words a user sees: branch, submission, checkout,
combination, blamed, result, test, parallel, batch, state. The rules
page keeps the statement's design words, since it is core.md's list. Nothing
commercial appears here (rule core.13).
