---
name: setting-up-sluice
description: Install sluice and set it up in a repository. Use when the user asks to install, set up, configure or adopt sluice, write or change `.config/sluice.toml`, explain or read back `.config/sluice.toml`, gate a branch, move from `wt merge` or direct merges to a merge queue, land on GitHub through sluice, give sluice a GitHub App, or take pull requests through it, or set up sluice's herdr plugin: its keys, sidebar rows, tab bar and toasts in herdr's `config.toml`; also when `sluice` is not installed or is too old. Chooses the runner, executor, worktree driver and forge for the tools the repository already uses (moon, worktrunk, devenv, GitHub) and shows a reference example for each.
---

# Setting up sluice

Work through these in order. Show every command before running it, and ask
before anything that installs software, changes host config, commits, or
touches GitHub settings.

## 1. The binary

```sh
command -v sluice && sluice --version
```

This plugin needs **sluice 0.12 or later** (`sluice overview`). If it is
missing or older, install or upgrade it. Pick the first that fits:

| the host has | install | upgrade |
|---|---|---|
| sluice from a flake input or home-manager (path under `/nix/store` or `/etc/profiles`) | bump the input in the user's own flake and rebuild; it is their config, so tell them, don't edit it unasked | same |
| nix | `nix profile install github:agentplot/sluice` | `nix profile upgrade --all` (or the entry's name from `nix profile list`) |
| neither, macOS on Apple silicon or Linux x86-64/ARM | the release tarball, below | the same command again |
| Rust 1.98+ | `cargo install --locked --git https://github.com/agentplot/sluice sluice` | the same with `--force` |

```sh
# targets: aarch64-darwin, x86_64-linux, aarch64-linux
mkdir -p ~/.local/bin
curl -fsSL https://github.com/agentplot/sluice/releases/latest/download/sluice-aarch64-darwin.tar.gz \
  | tar -xz -C ~/.local/bin sluice wt-submit
```

Each way installs `sluice` and `wt-submit` (worktrunk runs it as `wt submit`).
Check `~/.local/bin` is on `PATH`, then `sluice --version` again.

## 2. A working copy per branch

sluice is set up in a repository whose git directory stands apart from its
working copies, one per branch, so every session works and submits from a
working copy of its own (126). Before surveying or writing anything, ask
the conversion how the repository stands, from where you were asked:

```sh
bash "${CLAUDE_PLUGIN_ROOT}/scripts/lay-out.sh" kind .
```

It changes nothing and prints one line. `<the folder>` is the folder that
holds the git directory, `.git`, wherever you were asked; sluice's own
checkouts under the git directory, and worktrees whose folder is gone,
count as no working copy.

- `apart`: the git directory already stands apart from every working
  copy, as in a worktree of a bare git directory. Make no offer; go on to
  step 3.
- `only <the folder>`: the folder is a plain clone, the repository's one
  and only working copy. Offer the layout before anything else, and wait
  for the user's choice: [the only working copy](#the-only-working-copy).
- `several <the folder>`: the git directory is inside one of several
  working copies. Make the same offer and go on without waiting:
  [beside other working copies](#beside-other-working-copies).
- Exit 1: where you were asked is no working copy of a git repository.
  Say so, and ask the user where the repository is.

### The only working copy

1. Run the plan, which changes nothing, naming this session's process:

   ```sh
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/lay-out.sh" plan <the folder> --session-pid "$PPID"
   ```

   Write `--session-pid "$PPID"` into the command line itself: the shell
   that runs it expands `$PPID` to its parent, this session's process.
   The script then never counts this session, or anything it started
   (the shell the command runs in, a tool server, a watcher), as holding
   the folder, so it neither shows them nor refuses on them.

   When worktrunk is used, check its `worktree-path` template as the
   `worktrees` skill's layout reference says, "worktrunk", as part of
   the plan.
2. Show it to the user in plain words, the costs first:
   - each process the plan says holds the folder, by its command and id,
     which must be stopped, or close the folder, and be started again
     from `<the folder>/main`; this session and what it started are
     never among them;
   - every editor or app to close the folder in and reopen at `main/`:
     an editor that only watches the folder shows in no listing;
   - the Claude Code history kept under the folder's path, and the paths
     it will be carried to;
   - what stays under the old path: prompt recall, and the folder's trust
     and permission answers, which Claude asks again once for `main/`;
     sessions started in a subfolder keep their own, and
     `claude --resume <session-id>` still finds them.

   Then the layout: the git directory kept bare in the folder as
   `<name>.git`, the working copy moved into `main/` on its branch with
   its changes, and a folder per branch beside it.
3. Ask one question: lay it out, or keep the plain clone. Write nothing
   and commit nothing until the user answers.
4. On "lay it out", run the conversion, naming this session by its id,
   so its own transcript is left for `/cd`, and by its process, as the
   plan did:

   ```sh
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/lay-out.sh" go <the folder> --session "${CLAUDE_SESSION_ID}" --session-pid "$PPID"
   ```

   - Exit 1: it refused and moved nothing. Name what holds the folder, as
     its output does, and wait for the user's word that it has let go
     before running `go` again. Never move anything by hand.
   - Exit 2: a step failed after the move began. Stop and show the user
     its output: every file is still on disk and nothing was deleted (the
     `worktrees` skill's repair reference).
   - Exit 0: tell the user what it carried and what it left, then ask them
     to type `/cd <the folder>/main`, which moves this session there. Go
     on to step 3 in `main/`, never in the folder, which now holds the git
     directory.
5. On "keep the plain clone", go on to step 3 in the clone. Step 5's
   commit records the choice.

### Beside other working copies

Every session can already work from a working copy of its own here, so
the setup offers the move and goes on as the repository stands (126).

1. Run the plan on the folder `kind` named, as for the only working copy,
   naming this session's process, and, when worktrunk is used, with its
   `worktree-path` template checked as part of it:

   ```sh
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/lay-out.sh" plan <the folder> --session-pid "$PPID"
   ```

2. Show it to the user as a plain clone's plan is shown, the costs first,
   then the layout. Where it exits 1, its `go refuses now` lines are what
   stops the move today, such as a worktree inside the folder: say so,
   and why.
3. Then say, in your own words, that every session can already work from
   a working copy of its own, so the setup goes on with the repository as
   it stands, and that you lay the folder out whenever the user asks. Ask
   nothing about the layout: no question, no choice offered, nothing that
   ends your turn waiting for an answer.
4. Go on to step 3, in the same turn, in the working copy you were asked
   in, whether the folder or a worktree beside it, and write and commit the
   policy there as for any repository.

Whenever the user says to lay the folder out, at any point in the setup,
run `go` on the folder as on "lay it out" for the only working copy,
with `--session` and `--session-pid`, and take its exits 1, 2 and 0 as it
does, but for `/cd`: on exit 0, ask for `/cd <the folder>/main` only when
you were asked in the folder. Asked in a worktree beside it, that
worktree was repaired in place: go on there. A policy written in the
folder and not yet committed moves into `main/` with the working copy's
other changes, as the plan says of every changed path. Then carry on
with the step you were on.

## 3. Survey the repository

Find out what the repository already uses before writing anything:

| look for | means |
|---|---|
| `.moon/workspace.yml`, `moon.yml` | runner `moon` ([profiles/moon.md](references/profiles/moon.md)) |
| a single test command (`make test`, `npm test`, `cargo test`, `pytest`) and no moon | runner `command` ([profiles/command.md](references/profiles/command.md)) |
| `devenv.nix` / `devenv.yaml` | `toolchain = ["devenv", "print-dev-env"]` |
| `flake.nix` with a dev shell, no devenv | `toolchain = ["nix", "print-dev-env"]` |
| `.prototools` | moon and tools pinned by proto; sluice runs `proto install` for you |
| tests that start docker, compose or testcontainers | `[executor] docker = true`, a `[tests] release`, and a test that starts the engine's containers, waits for them, and exits with a `[tests] not_run` code when it cannot ([profiles/checkout.md](references/profiles/checkout.md)) |
| tests that listen on ports | `[checkout] ports` ([profiles/checkout.md](references/profiles/checkout.md)) |
| `.config/wt.toml`, `wt` on `PATH`, `wt merge` in scripts | worktrunk driver ([profiles/worktrunk.md](references/profiles/worktrunk.md)) |
| `git remote get-url origin` on github.com, and the target branch lives there | `remote = "origin"` ([profiles/remote-branches.md](references/profiles/remote-branches.md)) |
| reviews or other systems' checks must pass before landing | pull requests ([profiles/pull-requests.md](references/profiles/pull-requests.md)) |
| a stack a person should try after each landing | previews ([profiles/previews.md](references/profiles/previews.md)) |

Before proposing any branch or pattern, read how the project already
names its branches, and follow it:

```sh
git branch -r                                   # the branches the remote holds
grep -n '^\[branches' .config/sluice.toml       # the patterns a policy already names
ls CONTRIBUTING* docs/CONTRIBUTING* 2>/dev/null # where a project writes its conventions
```

Propose only names the project uses or its conventions describe. The
patterns in this skill and its references, such as `feature/*`, are
placeholders that show the shape of a table; never offer them as a
scheme to adopt.

Then ask the user, before writing anything:

1. **Which branches agents will submit to.** A name (`main`), or a family
   of them, written in the project's own naming (for example
   `<prefix>/*` for integration branches that take small fast landings).
   Each becomes a `[branches."<pattern>"]` table, and only sluice moves
   those branches.
2. **For each, what must pass before work lands there.** In a moon
   repository, offer the tags its tasks carry:

   ```sh
   moon query tasks | jq -r '[.tasks[][].tags[]?] | unique | .[]'   # every tag; then --tags <tag> for its tasks
   ```

   A fast tag such as `#check-fast` suits a family of integration branches, and a slower
   one such as `#e2e` can be added on `main` only.
3. **For each that lives on GitHub, whether the team lands on it through
   pull requests today.** Ask only for a branch whose policy will name a
   remote on GitHub: one kept on this host has no pull requests, and
   sluice refuses the table without `remote`, and in a policy with no
   `[signers]`, whose branches' state each host keeps. Ask per branch, since a team
   often reviews everything into `main` and lets agents land on a family
   of integration branches straight from their worktrees. On no, write no `pull_requests` table
   for it; on yes, see [a branch the team lands by pull
   request](#a-branch-the-team-lands-by-pull-request).

Tell the user what their answer means. `[tests]` picks how checks run;
each branch's `required` picks which. Under the command runner the one
command runs in full on every gated branch, so a quick suite on `dev/*`
and the full one on `main` need moon, whose tasks each branch names.

### A branch the team lands by pull request

Read what GitHub requires of the branch now, from its rulesets and from
classic branch protection; for a pattern, read an existing branch that
matches it. `gh` fills in `{owner}` and `{repo}` from the remote:

```sh
gh api repos/{owner}/{repo}/rules/branches/main --jq '.[] | {type, parameters}'
gh api repos/{owner}/{repo}/branches/main/protection   # 404: no classic protection
```

- From the rules: the `pull_request` rule's `required_approving_review_count`
  and `require_code_owner_review`, the `required_status_checks` rule's
  `required_status_checks[].context`, and whether there is a
  `merge_queue` rule.
- From protection: `required_pull_request_reviews.required_approving_review_count`
  and `require_code_owners_review`, and `required_status_checks.checks[].context`.

Show the user what was read, and from where, then offer:

- `reviews`: the highest approving review count of them all;
- `checks`: each check's name, once, leaving out `sluice` and every name
  starting `sluice / `. Those are sluice's own, posted on what it tests,
  and a pull request never waits for them.

Where `gh` is missing or signed out, or a read is refused (protection
needs admin rights), say which read failed and ask the user how many
approving reviews and which checks the team requires.

Then say plainly what the policy can't hold. The table has `reviews` and
`checks` alone, so a review from code owners has no condition there, nor
has the approval of the latest push, resolved conversations, a
deployment or code scanning. For each the branch requires, say: "sluice's
policy has no condition for a review from code owners, so sluice will not
wait for one: a pull request lands once it has its reviews and checks and
its combination passes." Never stand in for one, such as by counting code
owners into `reviews`: sluice would land a pull request the team's own
rule would have held.

Write what the user confirms. A branch that takes pull requests keeps its
state on its remote, where the host that keeps it takes its requests in,
so propose `[signers]` with the table where the policy names none: a
signer for the hosts whose runs count on the branch, and `trust` naming
it ([peers](references/profiles/peers.md) sets up the keys). Without
signers, sluice refuses the branch alone: `main takes pull requests,
which only the host that keeps it on origin takes in, and its state is
each host's own: name [signers] in .config/sluice.toml so its state is
kept on origin (137)`.

```toml
[signers.box]
ssh = ".config/sluice.boxes"   # one line per host's key

[branches.main]
trust = ["box"]

[branches.main.pull_requests]
reviews = 1
checks = ["ci/test"]
```

Before the policy is committed, tell the user what keeping their pull
requests means on that branch:

- Pull requests are opened as always: by an agent's `sluice submit` or
  `wt submit`, or by hand.
- A draft waits until it is marked ready for review; a ready one waits
  for the reviews and checks the table names.
- Then sluice tests it with what is ahead of it and lands it with its own
  push, and GitHub marks it merged.
- Nobody presses merge, uses GitHub's merge queue or merges by hand on
  that branch. Only who lands a pull request changes, from the merge
  button to sluice's push.
- A pull request opened by hand has no submit telling sluice of it:
  sluice's host process takes it in at its passes while it runs for the
  repository, and any command of sluice's there starts it. `sluice step
  <branch>` takes the branch's pull requests in at once.

sluice has no mode in which GitHub's button lands and sluice only tests.
Whenever the user asks for one (keep the merge button, have sluice only
check pull requests, require its check in the ruleset), say there is
none, and why: what the button lands was never tested with what landed
just before it, so two pull requests that each passed alone can break
the branch together. Offer the pull request conditions instead, and
never improvise such a mode, such as sluice's checks required in a
ruleset with the button left on.

## 4. Choose, per part

| part | choices today | reference |
|---|---|---|
| how tests run (`[tests] runner`) | `moon`, `command` | [moon](references/profiles/moon.md), [command](references/profiles/command.md) |
| what tests may touch (`[executor] kind`) | `open`, `sandbox` (macOS) | [sandbox](references/profiles/sandbox.md) |
| where a combination is tested (`[checkout]`) | a clean git worktree, with ports and a release command | [checkout](references/profiles/checkout.md) |
| who submits | `sluice submit`, `wt submit` (worktrunk) | [worktrunk](references/profiles/worktrunk.md) |
| where the branch lives | local, or on a remote (`remote`) | [remote-branches](references/profiles/remote-branches.md) |
| sluice's identity on GitHub (host `[forge]`) | a GitHub App | [github-app](references/profiles/github-app.md) |
| how work reaches the queue | straight from a worktree, or pull requests | [pull-requests](references/profiles/pull-requests.md) |
| running copies of landed revisions (`[preview]`) | a command per repository | [previews](references/profiles/previews.md) |
| more than one host, or more than one system (`[signers]`, `[systems]`) | peers on the git remote, standing hosts signing with SSH keys | [peers](references/profiles/peers.md) |

The rules per branch (`required`, `parallel`, `batch`, `hold`, `flaky`,
`timeout`, `release_retries`, `trust`) and `[landing] subject` are in the
[configuration reference](https://agentplot.github.io/sluice/reference/configuration/#branchespattern-rules-per-target-branch).
Leave `flaky = "fail"`.

### Parallel, batch and the host's bound

Never leave `parallel` and `batch` to their defaults unexamined: they decide
how long agents wait and how much the host runs. Find out, for each gated
branch:

1. **How long its gate takes.** Time the required checks once, from CI's
   history, the repository's own test script, or by running them.
2. **How often submissions arrive.** Ask how many agents will submit to it.
3. **What one run costs the host.** Tests that start containers, an
   emulator or a database hold that stack for the whole run.

Then choose, and tell the user why:

| the branch | set |
|---|---|
| a gate of a few minutes | `batch = false`, `parallel = 2` to `4` |
| a gate of 15 minutes or more, with several agents submitting | `batch = true`, `parallel = 4` to `8` |
| failures expected to be common | `batch = false` |

- `batch = false` tests every combination as it arrives, up to `parallel` at
  once: each submission lands about one run after it was submitted, and it
  costs one run per submission.
- `batch = true` tests only the longest combination, one run at a time on the
  branch. Submissions arriving meanwhile wait and form the next batch; no
  timer collects them. A pass lands the whole batch; a failure costs a second
  round, testing the shorter combinations, before the blamed submission is named.
  `parallel` is then the largest batch.
- One branch can't do both. A family of integration branches and `main`
  can differ: `batch = false` on that pattern with a short gate,
  `batch = true` on `main` with the full one.
- Set `timeout` to about twice the gate's usual time; the default is an hour.
- Set `hold` on a branch whose every move starts a build or a deployment,
  such as `main`, to how far apart submissions may arrive and still land in
  one move, e.g. `"5m"`. Leave it off integration branches that nothing deploys.
- Leave `release_retries` unset. A checkout whose release command fails is
  tried three more times, one a round, then held for the user to release
  with `sluice release <checkout>`; it never holds the branch.
- Set the host's `[runs] max` (step 6) to how many runs the host holds at
  once. It caps every branch of every repository on the host together,
  agents' `sluice exec` runs and previews too: every sluice on the host
  reads one count. With no `max`, a quarter of the cores sluice may use is
  shared by every repository on the host. Leave room in it for what else the
  host runs, agents' sessions included; sluice reads nothing of how busy the
  host is. On a Mac, which gives sluice every core, set `[runs] cores` too,
  to the cores sluice may use, so the runs leave the rest to the sessions.
- Every run gets its own checkout and its own ports. Check the tests write
  nothing outside the checkout (a fixed output directory, a lock file, a port
  they choose themselves): two overlapping runs collide there and fail
  unrelated submissions.

After a failure sluice tests fewer at once, halving the number for each
blamed submission and adding one back per pass; `sluice status <branch>`
shows `testing N at once (up to M)`. A branch stuck at one is a branch whose
submissions keep failing.
[Parallel and batch](https://agentplot.github.io/sluice/guides/parallel-and-batch/)
draws all of this.

Complete starting points:
[moon-minimal](references/examples/moon-minimal.sluice.toml),
[moon-worktrunk-github](references/examples/moon-worktrunk-github.sluice.toml),
[moon-peers-github](references/examples/moon-peers-github.sluice.toml),
[command-only](references/examples/command-only.sluice.toml).
Every key: <https://agentplot.github.io/sluice/reference/configuration/>.

## 5. Write, read back and commit the policy

Where step 2 offered the layout of a repository's only working copy,
write nothing here until the user has answered it: without their choice,
no `.config/sluice.toml` is written there. Beside other working copies,
nothing here waits on the offer.

Write `.config/sluice.toml`. sluice reads it **from the branch it gates**,
never from a submission, so it takes effect once it is committed on that
branch.

Before committing it, read it back to the user by
[read-back](references/read-back.md): each branch pattern, what it matches,
what must pass there and what a pull request into it waits for. A branch
on which no test will run leads its part with the warning, in that page's
words; stop there until the user names a check for it or says it is meant
to land untested.

Then commit it the way the repository lands work today, before protecting
the branch. That one change is the last one landed by hand. When the user
kept the plain clone, the repository's only working copy, at step 2, its
message carries this line, the setup's record of their choice:

```
The operator chose to keep this plain clone over a git directory with
one working copy per branch (126).
```

Beside other working copies nothing was chosen, and the message says
nothing of the layout.

## 6. Protect the branch and set up the host

```sh
sluice protect        # git hook: only sluice moves a gated branch (submit, step and run install it too)
```

### The host's file

Host settings live in one file on the host, never in the repository:
`$XDG_CONFIG_HOME/sluice/config.toml`, `~/.config/sluice/config.toml` when
`XDG_CONFIG_HOME` is unset. Every repository on the host shares it. sluice
reads it as each command starts and never writes it; sluice's host
process takes a change up between its passes, so nothing restarts it, and
hands a change of `[telemetry]` to a fresh host process it starts itself.
Before writing it, find out who writes it:

```sh
f="${XDG_CONFIG_HOME:-$HOME/.config}/sluice/config.toml"
for p in "$f" "${f%/*}"; do [ -L "$p" ] && echo "managed: $p links to $(readlink "$p")"; done
[ ! -e "$f" ] || [ -w "$f" ] || echo "managed: $f is read-only"
[ ! -d "${f%/*}" ] || [ -w "${f%/*}" ] || echo "managed: ${f%/*} is read-only"
```

The host is one of three setups:

1. **The user administers the host themselves.** Nothing printed `managed`.
   Show the tables to add, and on the user's word make the directory
   (`mkdir -p`) and write the file.
2. **A tool manages the host's configuration** (Nix, Ansible, an MDM). The
   check printed `managed`, or the user says a tool manages the file. Say
   so in those words, naming the link's target or the read-only path, and
   write nothing: not the file, not the link's target, not a file put in
   the link's place. Show the tables to add, for the user to add where the
   host's configuration is managed and apply from there; an edit on the
   host is refused, or lost at the next apply. A key file stays out of any
   store everyone may read, as Nix's store is: sluice refuses a key anyone
   but its owner may read, so the key goes on the host by hand or through
   the tool's handling of secrets, mode `0600`, and the managed file names
   its path. Where the link points into the Nix store and the host's
   configuration imports sluice's own module (`darwinModules.sluice` or
   `nixosModules.sluice`), the tables go in `services.sluice.settings`,
   each key named as in the file (`runs.max = 3;`,
   `forge.key = "~/.config/sluice/app.pem";`), and a key file by its path
   as a string, never a Nix path, which evaluation refuses. Where the
   host's own tool gives the app's token, `forge.credential` names that
   command in place of `forge.key`, a list of its program and then each
   argument, `forge.credential = [ "<program>" "<argument>" ];` with both
   placeholders for the tool's own; evaluation refuses a string there and
   a token's text.
3. **Several hosts as peers.** Each host has its own file, checked and
   written on that host as 1 or 2 says. Each holds what is that host's
   alone: its signer and key, the systems it runs, its bound, its name
   where two hosts share one. The hosts share the branch's policy, which
   lists each host's public key ([peers](references/profiles/peers.md)).

What the file holds:

| table | sets | the host needs it when |
|---|---|---|
| `[runs]` `max`, `cores`, `reserved` | runs at once, the cores they use, runs kept for submissions | always; each has a default |
| `[runs] systems` | the systems the host runs | it runs only some of the policy's systems |
| `[forge]` | the GitHub App sluice pushes as, by its key or by a command that gives its token | it lands on a branch on GitHub ([github-app](references/profiles/github-app.md)) |
| `[signing]` | its signer and SSH key | it runs checks for a branch whose state is on its remote ([peers](references/profiles/peers.md)) |
| `[peers]` | `lease`, `alive`, `name` | it shares a branch's remote with other hosts |
| `[[caches]]` | the caches every run on the host shares | its builds share a cache ([checkout](references/profiles/checkout.md)) |

The caches a run shares are the host's, never a policy's: each `[[caches]]`
names a `dir` outside every checkout, its server's `socket` where it has
one, and the `env` its clients read. Every run sluice starts on the host,
a submission's, an agent's `sluice exec` and a preview's command alike,
gets each `env` exactly as written, so the agents' shells need the same
values; on a managed host, set both from the same configuration. sluice
starts no cache's server.

Every host has a bound, in `[runs]`:

```toml
[runs]
max = 2         # runs at once on this host, every repository's; default a quarter of the cores
cores = 12      # the cores sluice may use; default what the host gives it, every core on a Mac
reserved = 1    # of those, kept for submissions; agents' `sluice exec` never takes them
```

Each run is given the cores sluice may use divided by `max` (`SLUICE_JOBS`,
and moon's `--concurrency`), so the runs going at once, whatever their
repositories, use those cores and no more. Without `cores`, sluice may use
what the host gives it: its CPU affinity or cgroup quota on Linux, and every
core where the host sets neither, as on a Mac. sluice refuses a `cores` below
1, above the host's cores, or on a host that already limits it.

A repository whose tests bring up a stack of containers sets `max` to how
many stacks fit in memory at once.

### A branch on GitHub, and several hosts

For a branch on GitHub, do these in this order, or a step is refused:

1. Commit the policy, with `remote = "origin"` on the branch, and push the
   branch to GitHub by hand, once ([remote-branches](references/profiles/remote-branches.md)).
2. Add the branch's ruleset with the user's own account in its bypass list.
3. Create and install the GitHub App, and name it in `[forge]`
   ([github-app](references/profiles/github-app.md)).
4. For a branch with a `pull_requests` table, now that the policy holds
   its conditions: propose removing the required approvals and status
   checks from its ruleset, or its classic branch protection, and turning
   GitHub's merge queue off. The conditions belong to the policy, read
   from the branch so a pull request can't loosen them, and sluice is the
   queue: GitHub's would land what sluice never tested. Make each change
   only on the user's word.
5. Put the app in the bypass list, then remove every other entry.
6. `sluice forge check <branch>`.

For more than one host or system, follow [peers](references/profiles/peers.md)'s
"Set it up, in order": a signing key per host, `[signing]` in each host's
file (checked first on each host, as *The host's file* says), then one
landing that adds `[signers]` and rewrites `trust`, then each standing
executor started as that page says.

A pattern with `remote = "origin"` needs no push by hand: `sluice branch
create <branch> --from main` makes each branch under it there as the app,
and its ruleset restricts creations and deletions as well as updates.

## 7. Check it end to end

1. Make a worktree on a new branch, commit a harmless change.
2. `sluice submit --check <branch>` says no conflict.
3. `sluice exec <branch>` runs the gate's tests in the worktree and passes.
4. `sluice submit <branch>` lands it; `git log -1 <branch>` shows the
   `Sluice-*` trailers.
5. `sluice overview` shows the landing under `recent`.

Then tell the agents: they submit, never merge (the `using-sluice` skill).
Remove any merge step, pre-merge test hook or post-merge check from their
scripts: sluice runs the gate on the exact combination that lands.

## What does my policy do?

When the user asks what an existing `.config/sluice.toml` does, or why a
branch landed something untested, give the same read-back
([read-back](references/read-back.md)), warnings first for their
branches.

## herdr's plugin

When the user asks to set up sluice's herdr plugin, its keys, sidebar
rows, tab bar or toasts, follow [herdr](references/herdr.md). It needs no
repository: of the steps above, only the binary's, since the plugin runs
`sluice`.

The asking is the user's word to edit herdr's own `config.toml`: the
opening rule's ask before changing host config is met for that file, so
ask nothing more before writing it, and say what you added afterwards. The
rule still holds for the rest: ask before installing the plugin, and
before changing a line of sluice's the file already holds.
