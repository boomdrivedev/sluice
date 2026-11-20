// Writes the site's rules page, src/content/docs/architecture/rules.md,
// from the rule list of docs/architecture/core.md, each rule's id linked to
// core.md's details. Run before every build and dev (package.json); the
// page is not committed (record 0127).
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = resolve(here, "../../docs/architecture/core.md");
const target = resolve(here, "../src/content/docs/architecture/rules.md");
const blob = "https://github.com/boomdrivedev/sluice/blob/main/docs/architecture/core.md";

const text = readFileSync(source, "utf8");
const start = text.indexOf("\n## Rules\n");
const end = text.indexOf("\n## Details\n");
if (start < 0 || end < start) throw new Error("core.md: no `## Rules` section before `## Details`");
const rules = text
  .slice(start + "\n## Rules\n".length, end)
  .trim()
  // Each group of rules is a section of the page.
  .replace(/^### /gm, "## ")
  .replace(/\*\*\[(core\.\d+)\]\*\*/g, (_, id) => `**[${id}](${blob}#details)**`);
if (!/\*\*\[core\.\d+\]\(/.test(rules)) throw new Error("core.md: no rule in `## Rules`");

const page = [
  "---",
  "# Written by site/scripts/rules.mjs from docs/architecture/core.md at each",
  "# build and dev; change core.md.",
  "title: Rules",
  "description: The rules that bind every change to the repository, cited by clause and decision.",
  `editUrl: https://github.com/boomdrivedev/sluice/edit/main/docs/architecture/core.md`,
  "sidebar:",
  "  order: 2",
  "---",
  "",
  `The rules of [docs/architecture/core.md](${blob}), the binding copy,`,
  "as it lists them. A reviewer can point at a diff and say which rule it",
  "breaks. Each rule's id links to its details in core.md: what it rules",
  "out, what it allows and the clauses and decisions it rests on.",
  "",
  rules,
  "",
].join("\n");

writeFileSync(target, page);
console.log("rules: written from docs/architecture/core.md");
