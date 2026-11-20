---
number: 19
title: sluice pushes as a GitHub App whose token reaches only its own git
status: accepted
date: 2026-10-14
decision-makers:
- Chuck Swanberg
---

# sluice pushes as a GitHub App whose token reaches only its own git

Deciders: the user, planning the change `github-app` and ruling on how `sluice forge check` reads a ruleset's bypass list, 2026-10-14.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clause C.4; `design/sluice/proposals/many-hosts-2.md` (C.4: "an identity no session on any host can use"); ADRs 0003, 0017.

## Context and problem statement

Since ADR 0017, GitHub's `main` is the real one, and sluice lands on it by a push with the host's own git credentials. Agents run git as the same user with the same credentials, so an agent's `git push origin HEAD:main` reaches GitHub's `main`: the local `reference-transaction` hook guards only the local branch. C.4 asks for the forge's rules, which only sluice's identity bypasses. Which identity does sluice push as, and how does it keep that identity from the agents beside it?

## Considered options

1. The operator's account, the ruleset's bypass, as now.
2. A machine user's fine-grained personal token, the ruleset's bypass.
3. A GitHub App, the ruleset's only bypass, whose installation token sluice gives only to its own git calls.

## Decision outcome

Option 3. The host's config names the app in `[forge]`: its id and the path of its private key, a file with no group or other permission bits. sluice signs a ten-minute JWT with the key (RS256), finds the app's installation on the repository, and buys an installation token for that repository, good for an hour. A process keeps the token in memory and buys another five minutes before it ends.

The pushed refs profile takes an optional identity, an interface in the sluice crate that the GitHub profile implements (rule core.2), so the profile asks for credentials and does not know they are GitHub's (rule core.1). With an identity, each fetch and push runs as `git -c credential.helper= -c credential.helper='!<sluice> credential' …` over the remote's HTTPS URL, its configured URL before any `insteadOf`. The empty helper clears the user's helpers for that call alone; `sluice credential get` answers git with the token, which it finds in that git call's environment and nowhere else. Nothing is written to a git config, so the git an agent runs never finds the helper, carries the user's own credentials, and is refused by the ruleset. Every fetch and push sluice makes, with an identity or without, runs with no terminal prompt, no askpass program and a credential manager told never to ask: sluice runs unattended, so a credential it cannot find is an error. With no `[forge]`, sluice pushes with the host's credentials, as ADR 0017 does, and `sluice status` says the branch has no identity of its own.

`sluice forge check` verifies the setup: the key reads, a token mints, the installation may write contents, and every ruleset on the branch has the app, always, as its only bypass. GitHub shows a ruleset's bypass list only to a caller with write access to the ruleset, and an app that could edit the ruleset could let anyone through it, so the check reads the rulesets with the operator's own token (`GH_TOKEN`, `GITHUB_TOKEN` or `gh auth token`) and everything else with the app's.

Option 1 is what lets an agent's push through: the agents hold the operator's credentials. Option 2 is a second account to manage, its token a secret like the key, and a personal token cannot create the check runs the `pull-requests` change needs. An `http.extraHeader` carrying the token instead of a helper was rejected: it shows in the process list for the call's lifetime.

### Consequences

- The key is a file the host's user may read, and agents run as that user: the ruleset keeps agents off `main` because their git never uses the key, not because they cannot read it. Agents under another user, or the key in a secrets manager, would close that; both are later.
- A token that cannot be bought, GitHub down or the key revoked, fails the round as a failed fetch does: it lands nothing and says why.
- The tests sign with a key made for them, run git through the helper against a fake HTTP remote, and check the app against a fake of GitHub's API on a port the system picks; no test reaches the user's git config or credentials.
- The live test against a scratch app and repository is the operator's, run once by hand; it has not been run.

## Amendment of 2026-11-18: a landing names the App, and the check covers every permission sluice uses

Decider: the design agent, on the open list of `docs/plans/phase-1-pass-2.md` (piece 25 and its follow-ups), as the review of 2026-11-18 sent it, and on boomdrivedev/boomdrive record 0007, whose one App the family lands with and which counts "GitHub shows the product's name on every landing" among its consequences.

Landing commits are written as `sluice <sluice@localhost>`, fixed in the code, so GitHub links no landing to the App that pushed it, whatever record 0007 expects.

1. **A landing names the App.** Where a branch's records are on a forge that sluice reaches as a GitHub App, every commit sluice writes for it, a landing's and a records commit alike, has the App's own bot account as its author and committer: the App's name followed by `[bot]`, at `<bot user id>+<app slug>[bot]@users.noreply.github.com`, read from GitHub with the App's own credential and kept on the host. GitHub then shows the App on every landing. A branch with no App keeps `sluice <sluice@localhost>`.
2. **One App for every organization a host lands for.** A host's one `[forge]` App, installed on each organization it lands for, buys each repository's token from that repository's own installation, as record 0007 has it. Piece 25's entry per organization, each with its own App and author, is not taken: each organization already revokes the App by uninstalling it.
3. **The check covers every permission sluice uses.** `sluice forge check` reports each permission sluice uses of the App on the branch: contents, workflows, checks and pull requests to write, as it does, and commit statuses to read where the branch takes pull requests, since a request's conditions read other systems' statuses (record 0071).

**Options not taken.**

- *An organization's official name and address as a landing's author.* That is a person's or a company's identity on a commit no person wrote, and GitHub links it to no App.
- *An App per organization (piece 25).* Each organization would make and hold its own App and key, which record 0007's one App replaces.

## The build's choices within the amendment of 2026-11-18

Status: the construct stage's own choices within point 1 of the amendment above, made on 2026-11-18 for the OpenSpec change `every-merge-names-the-app-as-its-author`, whose design gives each with its reasons. They are cheap to change. The statement's C.4 names no author for a commit sluice writes, so nothing there changes.

- **A commit names the identity that pushes it.** Whether a commit is the App's is read where the commit is written, from the identity sluice reaches the commit's remote as at that moment, as every fetch and push reads it (129). A landing on a branch sluice pushes to GitHub as the App, and a records commit on a remote it pushes to as the App, name the App's bot. A branch in the host's own refs, one pushed with the host's credentials, and a remote on no forge the App is on name `sluice <sluice@localhost>`. The pushed refs profile asks its identity for the author through the identity's own interface, which the GitHub profile implements, so the profile names no forge (rules core.1, core.2).
- **The branch seam's commit names its branch.** The seam's `commit` takes the branch the landing is written for, as its reads and moves take theirs, and the instance's router writes the landing through the profile that branch is routed to, so a landing on a branch sluice pushes to GitHub as the App names the App's bot. The commit itself carries no branch: where it is written is not part of what is written (rule core.2).
- **A combination's commit stays sluice's.** A combination's commit keeps `sluice <sluice@localhost>` and the time 0 on every host, whatever identity the host has, so one combination forms one commit wherever it is formed (rules core.6, core.17, core.58). GitHub shows no combination on a branch: the landing commit carries the combination's tree on parents of its own.
- **The bot is read with the App's own credential.** Its login is read as the author of the pull requests sluice opens is read: under a key, the App's slug from `GET /app` with its JWT, followed by `[bot]`; under a credential command, GraphQL's `viewer` with the repository's token. Its user id is `GET /users/<login>` with the repository's token, under the retry policy and the installation's allowance (rule core.29). The author is the login, at `<id>+<login>@users.noreply.github.com`. A login that does not end in `[bot]` is no App's bot, and the commit that needed it fails, naming it.
- **Kept on the host for a day.** The login and the id are kept in the host's state, `forge/authors/<app id>.json`, for every process of sluice on the host, and read from GitHub again once a day has passed since they were read, so a renamed App is named by its new name within a day. Where that read fails, the kept login and id stand until a read succeeds, so a spent allowance or GitHub's outage holds no landing that a push would make.
- **A read that fails writes nothing.** Where the host keeps no bot for the App and it cannot be read, the commit that needed it is not written: a round lands nothing and says why in GitHub's words, as when a token cannot be bought. No commit is written under sluice's name in its place.

Not taken:

- *The branch on the landing commit*, filled in by the core: a commit would carry where it goes beside what it is, and the seam would name a branch one way there and another in its other calls.
- *The App's bot on a combination too*, as "every commit" reads by its letter: a host with the App and a host without it would form two commits of one combination.
- *The bot read once in each process*, as the pull requests' author is: every process, each command's and the host process's, would ask GitHub at its first commit, where the amendment keeps the author on the host.
- *Kept for good*: a renamed App would be named by its old name until someone removed the file.
- *A kept bot dropped once a day has passed*: a read that failed then would hold every landing until GitHub answered, though the bot it would read is the one kept.
- *GraphQL's `viewer { databaseId }`* for the id under a credential command: one read fewer there, but two ways to one value.
- *sluice's own name where the read fails*: a landing pushed as the App and named `sluice` is what the amendment rules out.

## Amendment of 2026-11-18: git sends no header its configuration stores, and a refusal names what git sent

Decider: the code stage of `fix/main--the-sign-job-pushes-its-record-as-the-app`, its own choice within the decision above, cheap to change. Its brief: "the workflow sluice init actions writes must keep the job's token out of git (persist-credentials: false on the sign job's checkout, or sluice's push overriding any stored credential), so the record is pushed as the App; and sluice's message must name the credential git actually sent, not blame the App's token."

On sluice 0.97.4, in boomdrivedev/sluice-actions-trial, both pool runs passed and neither sign job saved its record. `sluice job record` failed with `remote: Permission to boomdrivedev/sluice-actions-trial.git denied to github-actions[bot]. ... The requested URL returned error: 403`, and then said `origin refused app 5719968's token ... scoped to ... contents: write`, five times. `actions/checkout` keeps its job's token in the repository's git config, as a header for `https://github.com/`. git sends a stored header on every request, so GitHub never asked for credentials and git never asked sluice's helper. Emptying the user's helpers, as decided above, leaves such a header in place. So the push went as the job, which may only read, and sluice blamed a token it never handed over.

1. **git sends no header its configuration stores.** Every fetch and push sluice makes as its identity empties the headers git's configuration holds for the address it reaches the remote by, `-c http.<address>.extraheader=`, beside the emptied helpers. It is set on the call's command line alone, the most specific match for the address and the last read, so it wins over the repository's, the user's and the system's settings. Nothing is written to a git config.
2. **A refusal names the credential git sent.** A call as sluice's identity makes an empty file in the repository's git directory for the try and names it to the helper, `SLUICE_CREDENTIAL_ASKED`. `sluice credential get` marks it as it hands git the token. A refusal on a try whose file is unmarked is no refusal of sluice's token. The failure carries the remote's words, whose own account GitHub names, and says git sent a credential of its own in place of the identity's, never asking sluice for it. It names where git found that credential: the user name in the address git reached the remote by, once git's configuration rewrote it; the header git's configuration stores for that address, where a rewrite took git past the one emptied; or the entry the user's `.netrc` holds for the host, which git's HTTP library reads. It never names the secret. Where git got no credential at all from the helper, it says so instead. No token is bought afresh for it, since git would send the same again (134). The failure is the remote's answer, read once, as on the host's own credentials. The file holds no secret and goes with the try.

**Options not taken.**

- *`persist-credentials: false` on the sign jobs' checkout.* The sign job's `sluice job sign` and `sign-head`'s fetch of a pull request's head read the repository with the job's own token, which a private repository needs. And a header or a rewritten address that any host's configuration holds would still win over sluice's helper.
- *The App's token in a header sluice sets, in place of the helper.* The decision above rejected it, since it shows in the process list. It would also stand apart from every other tool of the family, which hands a token to git through a helper (boomdrivedev/boomdrive record 0007 as amended 2026-11-11).
- *Holding the address against the rewrites git's configuration names, `url.<address>.insteadOf` set to the address itself.* A rewrite is where CI often puts a token, but it is also how a host points the forge's address at a mirror or a proxy, and how sluice's own tests point it at a copy on the host. Holding it would send the App's token past them to the forge's own address. A token a rewrite carries is named when the remote refuses it, as above.
- *Telling a refusal of git's own credential by the remote's words.* GitHub names the account it refused, but another forge, a 404 or a 401 names none. Whether git asked the helper is the one sign that holds on every remote.
- *The mark on the helper's standard error, read back from git's.* It needs no file, but it would put sluice's own line among the remote's words, which are given whole (133).

### Consequences

- The record of a pool's run is pushed as the App with the workflow `sluice init actions` writes as it is, and so is any push of sluice's on a host whose git configuration stores a header for the forge.
- A token a host's rewrite of the forge's address carries, or a header stored for the address a rewrite reaches, is still sent in place of the App's. The refusal names it, and the host's configuration is the place to change.
- The family's other git callers that hand a token through a helper, the runner's among them, empty the helpers but not stored headers. They have the same gap where a host's configuration stores one for the forge.
- Rule core.29's details read with this amendment.
