---
number: 127
title: sluice's tasks read no Markdown
status: accepted
date: 2026-11-15
decision-makers:
- the user, in principle 16 of the family's principles
- the brief of the chore sluices-tasks-read-no-markdown
- the code stage of chore/sluices-tasks-read-no-markdown
---

# sluice's tasks read no Markdown

Status: accepted. The user's words decide that no check reads a Markdown file, and principle 16 says it. The brief decides which of sluice's tasks go and that the rules page is made at build. How the rest is done is the code stage's choice, each part cheap to change.

Deciders: the user, on 2026-11-11, on sluice's `root:installs`, a check that grepped the READMEs, skills and site pages for install commands: "I don't want fucking checks on instructions. That has got to fucking stop. That is my biggest pet peeve. We do not fucking write code tests against Markdown files." And on 2026-11-15, on their card: "OpenSpec's own validation stays; the rule is about checks of what pages say". The brief of the chore `sluices-tasks-read-no-markdown`, on the bolt `bolt/sluices-checks-read-no-markdown`: "root:adrs, root:openspec-claude and site:prose go, and the site's rules page is made from docs/architecture/core.md at build and no longer committed, so site:rules goes. root:specs, root:changes, root:toolchain, root:format and the plugins' validate stay." The code stage of `chore/sluices-tasks-read-no-markdown` chose the rest below.

Sources: the family's principles in boomdrivedev/blueprints, `design/principles.md`, principle 16 as amended 2026-11-15 (3a5c19d); rule core.21 as amended 2026-11-11 (4295ca8af); record 0101, which made `root:openspec-claude`; `site/scripts/adrs.mjs`, which makes the decision pages before every build and dev and commits none.

## Context and problem statement

Principle 16 says no check, test or gate task reads a Markdown file: a doc, a README, a skill, a site page, an instruction, a spec or a record. What a page says is kept by the people and agents who write and review it, never by code that greps it. A tool's own validation of the files it works on is no check of what a page says, and stays in the gate. Rule core.21 has held it since 2026-11-11. Four of sluice's tasks still read its Markdown:

- `root:adrs` read each record's frontmatter for its number, so no two records shared one.
- `root:openspec-claude` had the pinned OpenSpec write its Claude Code skills and commands into a scratch directory and compared them with the tracked ones (record 0101).
- `site:prose` ran Vale over the site's pages.
- `site:rules` compared the committed rules page with what `site/scripts/rules.mjs` makes from `docs/architecture/core.md`.

`root:specs` and `root:changes` run OpenSpec's own `openspec validate`, and each plugin's `validate` runs Claude Code's own `plugin validate`: each is a tool's own validation of the files it works on. `root:format`'s treefmt excludes Markdown. `root:toolchain` reads no Markdown.

## Considered options

For the rules page:

1. **The site's build makes it from core.md before every build and dev, as it makes the decision pages, and the tree does not track it** (chosen, the brief).
2. **The page committed and written again by hand when core.md changes, with no check.** It drifts from core.md the first time a writer forgets.
3. **No rules page; the site links to core.md.** The site loses a page a reader is sent to.

For the records' numbers:

1. **No check. A writer takes the next free number, and a reviewer sees two records with one** (chosen).
2. **`adrs.mjs` fails the site's build on a number two records share.** It is a check of a record's frontmatter by another name. And the build is the Pages workflow's, which runs once a change has landed: it would hold the site, never the change.
3. **`root:adrs` kept.** Principle 16 rules it out.

For OpenSpec's Claude Code files:

1. **The writer kept as `write-openspec-claude.sh`, run when the pin moves; nothing compares the files** (chosen).
2. **The files untracked and written at a place's readiness, as devenv writes Claude Code's settings (record 0101).** That changes every place's readiness and the tree's layout, which the brief does not ask.
3. **No script; a writer runs `openspec init` by hand.** Each writer would need the ten workflows in an OpenSpec configuration of their own, and might write the files from their own configuration's.

For the site's checks:

1. **The site holds no check** (chosen). Its pages are Markdown and it has no tests. Its build is the Pages workflow's.
2. **A type check of the site's components, `astro check`.** It needs the site's dependencies in every place, which no place holds, and is a check the brief does not ask.

## Decision outcome

Option 1 of each.

- **Four tasks go.** `root:adrs`, `root:openspec-claude`, `site:prose` and `site:rules` leave `moon.yml` and `site/moon.yml`, and `check-adrs.sh` leaves the tree.
- **The rules page is made at build.** `site/package.json`'s `prebuild` and `predev` run `rules.mjs` after `adrs.mjs`. It writes `site/src/content/docs/architecture/rules.md`, which `.gitignore` names, and has no `--check`. `site:build`'s inputs name `docs/architecture/core.md`.
- **OpenSpec's writer.** `write-openspec-claude.sh` refuses an `openspec` other than the release `devenv.nix` pins, has it write its Claude Code files into a scratch directory with the ten workflows sluice tracks, and replaces the tracked ones with them. Moving the pin is followed by `sh write-openspec-claude.sh` in the shell.
- **Vale stays a writer's aid.** `devenv shell -- prose` and the Claude Code hook run it for the writer who asks. No task of the gate runs it.
- **What stays in the gate.** `root:specs`, `root:changes`, `root:toolchain`, `root:format` and each plugin's `validate`.

### Consequences

- A change of core.md's rules no longer asks for the site's page to be written again: the next build makes it.
- Two records given one number reach main unless a reviewer sees them, and the site then shows one of them.
- OpenSpec's tracked files match the pinned release only as far as the last writer who moved the pin ran the writer.
- The site project holds no task in the gate's tiers, and core.21's one task per check holds with none.
- Rule core.21 holds the decision, and the specs `repository-gate` and `refused-writes` say it.
