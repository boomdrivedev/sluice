# Design

## Context

`sluice forge check` on GitHub (`crates/sluice/src/profiles/github/check.rs`)
reads the App's installation on the repository and reports four grants,
each to write. It names each missing grant with the grant GitHub's
settings page shows:

| grant | what sluice does with it |
|---|---|
| `contents` | lands, and pushes its records |
| `workflows` | lands a change under `.github/workflows` |
| `checks` | posts its check and its report's checks on a request's head, and reads other systems' check runs |
| `pull_requests` | opens, updates and marks requests, reads their reviews and timeline, and comments on them |

A pool on GitHub adds `actions`, which `check::pool` reports.

One grant intake uses is left out. Every branch that takes pull requests
reads other systems' commit statuses for a request's conditions. It
reads them through `StatusContext` in the GraphQL query `REQUESTS`, and
through `/commits/{rev}/status` (`rest.rs`; record 0071). sluice reads
statuses and never posts one: record 0021 turned down statuses as
sluice's own state. GitHub grants `metadata: read` to every App and it
cannot be taken away, so the check does not report it.

`fix/main--forge-check-reads-the-pool` makes `forge_findings` read the
policy whole. It reads it at the branch's revision as the host holds it,
with no fetch, or at main's for a branch not yet made, through
`whole_policy`. A policy it cannot read whole is a failed line in the
policy's own words. That fix reads the pools, so a pool on GitHub is
checked from the command line (record 0119 as amended 2026-11-18). This
change reads whether the branch takes pull requests from the same
policy.

## Goals / Non-Goals

**Goals:**

- `sluice forge check` fails a branch that takes pull requests when the
  App cannot read commit statuses, naming the grant.
- The guides ask for each grant sluice uses at the access it uses.

**Non-Goals:**

- The grants of a credential command's token, which GitHub does not show
  the token's holder (record 0108).
- Any change to how intake reads statuses or conditions.

## Decisions

### D1. The grant is read from the installation, as the other four are

`check` reads `statuses` from the installation's permissions, which it
already fetches with the App's key. `read` and `write` both pass, and the
line says `the app may read <repo>'s commit statuses`. Any other value,
or none, fails with
`the app may not read <repo>'s commit statuses (statuses: none): give it
Commit statuses read-only`. That is the form of the other four, with the
access sluice uses. The line follows the pull requests line, and the
four lines before it are unchanged.

Path not taken: probing statuses with a read of the branch's
`/commits/{rev}/status`. A probe spends a call of the installation's
allowance to learn what the installation already says. It also reads a
404 from a branch not yet made as a missing grant.

### D2. "Takes pull requests" is the table that gives the branch its gate

`forge_findings` reads the policy whole before it calls `check`. The
branch takes pull requests where `Policy::table_for(branch)` names a
table whose `pull_requests` is set. `table_for` gives the table whatever
`rules_for` refuses of it. A branch that takes pull requests with no
`[signers]` is refused by `rules_for` (137). Its table still says it
takes them, and the check reads that table as routing reads its remote
(core.27). Every round already names that refusal, so the check adds no
line for it.

Where the host holds no revision of the branch, the policy is main's,
which the fix reads for the pools. A `feature/*` table there that names
pull requests gives a branch not yet made the statuses line before the
branch exists, as the check already reads such a branch's remote and
rulesets.

Paths not taken:

- `rules_for(branch)`. It errs on the refused branch, and the check
  would then pass a branch whose requests read statuses once `[signers]`
  is named.
- Routing's reading, `Branches::policy_of`. `Policy::guarding` passes
  every table but the remote and the signers' names, so its
  `pull_requests` is never set (record 0069).

### D3. A policy not read whole leaves statuses unread

Where `whole_policy` refuses the policy, the check reads no statuses. The
refusal is already a failed line, `<branch>'s policy at <rev> is refused,
so its pools are not read: …`, and the check exits 1 on it. Once the
policy reads, the next check reports statuses.

`table_for` errs where two patterns tie for the branch. The check names
that as a failed line in its own words and reads no statuses. Every
round already refuses such a branch.

Path not taken: rewording the fix's line to name pull requests as well.
The fix's test holds those words, and the failed line already stops the
check.

### D4. The lines keep their order

The fix reads the policy after the rulesets and adds its refusal line
there. This change reads the policy before `check`, keeps the result,
and adds the refusal line where the fix adds it. The order of the lines
stays the same: the key or command, the token, the grants, the
rulesets, the policy's refusal, then each pool.

### D5. `check` takes whether the branch takes pull requests

`check(app, repo, branch, operator, requests)`. The new last argument is
a `bool`, named for what the policy says. It is not the policy itself,
because the grant turns on that one fact, which `forge_findings` already
reads from the policy. Under a credential command (`commanded`) the
argument is not read (record 0108).

### D6. The guides ask Commit statuses read-only

The site's `guides/github-app.md` and the setup skill's
`references/profiles/github-app.md` ask Commit statuses read and write
today. sluice only reads statuses, and the App's key is a file the
host's agents can read (record 0019, consequences). So the guides ask
read-only. The pull requests guide and the skill's pull requests
reference name it beside Checks and Pull requests. The command
reference names it among the grants the check reads, and among those a
credential command's check leaves to wherever the key is kept.

## Risks / Trade-offs

- An App set up from the guides before this change holds Commit statuses
  read and write. It passes, since write covers read. The guides now ask
  less, and nothing asks an existing App to give the extra access back.
- A host whose `[forge]` names a credential command still learns of a
  missing statuses grant only when intake fails on it (record 0108). The
  command reference says so.
