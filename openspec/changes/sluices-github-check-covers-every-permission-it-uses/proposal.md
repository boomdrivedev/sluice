## Why

`sluice forge check` reports four of the App's grants on a branch:
Contents, Workflows, Checks and Pull requests, each to write. A branch
that takes pull requests also reads other systems' commit statuses. Its
intake reads them for a request's conditions, through the one GraphQL
query per branch and through `/commits/{rev}/status` (record 0071). An
App installed without Commit statuses passes the check, then fails
every intake on that branch. The check said nothing about the grant
that intake needed.

Record 0019's amendment of 2026-11-18, point 3, has the check report
each permission sluice uses of the App on the branch. That covers
commit statuses to read wherever the branch takes pull requests.

## What Changes

- **The check reads Commit statuses where the branch takes pull
  requests.** With the App's key, `sluice forge check [<branch>]` reports
  the installation's `statuses` grant after the four it reports now. A
  grant to read or to write passes. No grant fails and names Commit
  statuses, read-only.
- **Whether the branch takes pull requests is read from the policy the
  check reads its pools from.** That is the policy read whole at the
  branch's revision as the host holds it, or at main's for a branch not
  yet made (record 0119 as amended 2026-11-18). The check reads the
  `[branches]` table that gives the branch its gate, whatever the rules
  refuse of that table, as routing reads the branch's remote (core.27).
- **The guides ask for what sluice uses.** The site's guides, the
  command reference and the setup skill's references give Commit
  statuses as read-only. They name it beside Checks and Pull requests
  where a branch takes pull requests, and say that `sluice forge check`
  reads it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-identity`: `sluice forge check` reports the App's grant to
  read commit statuses where the branch takes pull requests.

## Out of scope

- **A credential command's grants.** GitHub does not show a token's
  holder the token's grants. Under a credential command the check still
  reports only that the token comes, is the App's and may push (record
  0108), so it says nothing of Commit statuses there.
- **The landing's author.** Record 0019's amendment, point 1, names the
  App's bot account as the author of every commit sluice writes. That is
  the sibling change `every-merge-names-the-app-as-its-author` on this
  bolt.
- **Reading the policy whole.** `fix/main--forge-check-reads-the-pool`
  makes the check read the policy whole and name a policy it cannot read
  whole (record 0119 as amended 2026-11-18). This change waits on that
  fix: it lands on main, the bolt takes main, and this branch takes the
  bolt before any code here (task 1.1).

## Impact

- `crates/sluice/src/profiles/github/check.rs`: `check` takes whether
  the branch takes pull requests and reports `statuses`.
- `crates/sluice/src/profiles/mod.rs`: `forge_findings` reads the
  policy whole before `check` and passes whether the branch takes pull
  requests.
- `crates/sluice/tests/profiles/forge.rs`: a test of the grant, and one
  from the command line.
- Docs: `site/src/content/docs/guides/github-app.md`,
  `site/src/content/docs/guides/pull-requests.md`,
  `site/src/content/docs/reference/commands.md`, and the setup skill's
  `references/profiles/github-app.md` and
  `references/profiles/pull-requests.md`.
- Records and rules: record 0019 as amended 2026-11-18 (point 3);
  records 0071, 0108 and 0119 as amended 2026-11-18; rules core.15 and
  core.27. No rule changes.

## Touches

```
crates/sluice/src/profiles/github/check.rs
crates/sluice/src/profiles/mod.rs
crates/sluice/tests/profiles/forge.rs
site/src/content/docs/guides/github-app.md
site/src/content/docs/guides/pull-requests.md
site/src/content/docs/reference/commands.md
plugins/sluice/skills/setting-up-sluice/references/profiles/github-app.md
plugins/sluice/skills/setting-up-sluice/references/profiles/pull-requests.md
openspec/changes/sluices-github-check-covers-every-permission-it-uses/**
```
