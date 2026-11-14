---
title: Configuration
description: Every table and key of the policy file, the host's own config, and the environment.
sidebar:
  order: 1
---

A repository's policy is one file, `.config/sluice.toml`, checked in.
sluice reads it from the target branch, never from a submission, so a
submission cannot loosen the rules it is tested under.

```toml
[tests]
runner = "moon"
toolchain = ["devenv", "print-dev-env"]

[executor]
kind = "sandbox"
network = "localhost"

[landing]
subject = "chore(land): #{id} {from}"

[branches."feature/*"]
required = ["#suite"]
parallel = 4
batch = false
flaky = "fail"
timeout = "20m"
trust = ["open", "sandbox", "container"]

[branches.main]
required = ["#suite", "#live"]
parallel = 2
batch = true
hold = "5m"
trust = ["confined", "sandbox"]
```

## `[tests]`: how tests run

| key | values | meaning |
|---|---|---|
| `runner` | `"moon"` | Run moon tasks. sluice asks moon which tasks the combination affects and runs only those; a task whose inputs did not change replays its earlier result instead of running. |
| | `"command"` | Run one command; exit 0 passes. Every test runs every time. For a repository without moon. |
| `command` | a command | With `runner = "command"`, the command to run, e.g. `"tests/suite"`. |
| `release` | a command | The release command: what removes what the tests handed to another process to keep, a container above all. It runs once a checkout is released, under the same executor, toolchain and environment as the tests, ports included. It runs for a worktree too when `sluice release` runs in it, whether or not sluice holds anything for it ([ADR 0016](/sluice/decisions/0016/)). In a worktree sluice holds nothing for, the default branch's command runs, with no ports. One that fails holds the checkout; see [`release_retries`](#release_retries). A list, the command first, or a string run by `sh -c`. |
| `toolchain` | a command | Prints the shell the tests run in, e.g. `["devenv", "print-dev-env"]`. It is the first step of [equipping](#equipping-a-checkout) the checkout: it runs there, outside the sandbox, and every later step and the tests run inside it with what it printed. sluice reads nothing from it. A toolchain that fails runs nothing: no result is recorded, the checkout is released, and the combination is tested again, blaming nobody. After three runs in a row whose tests could not run, the combination is not passed, and each test is named with the toolchain's exit and what it printed. |
| `not_run` | an exit or a list of them | Exits by which a test says it could not run here, e.g. `[77]`. A test that exits with one has no result. Unset, every exit but `0` is a fail. A test that needs a container engine, a database or a stack starts it, waits for it within the branch's [`timeout`](#timeout), and exits with one of these when it could not set up. |
| `cold` | a list of tests | Tests that run with none of the host's [caches](#caches), e.g. `["#bench"]` for timings a warm cache would skew. Each is named as `required` names it: a moon target or a `#tag`, or the command with `runner = "command"`. A run that holds one gets no cache directory, no cache socket and none of the caches' variables, and its other tests lose only speed. |

Nothing in `[tests]` chooses what runs on a branch. With
`runner = "command"`, its command runs on every gated branch; with
`runner = "moon"`, each branch's [`required`](#required) names the tasks
that must pass there.

### Equipping a checkout

Before every run, sluice equips the checkout, outside the executor, with
the environment the run starts from. The `toolchain` runs first. Under
moon, `proto install --config-mode upwards` follows where the checkout
holds a `.prototools`, installing the tool versions it lists, then
`moon --version`, which has to say 2.6 or later, then `moon setup`.
Equipping stops at the first step that fails, and the run never starts.
A combination that affects none of its branch's required tasks runs
nothing, so it stops after `moon --version`. sluice equips an agent's
worktree before `sluice exec` and a preview's checkout before its
command the same way, and
[`sluice equip`](/sluice/reference/commands/#sluice-equip) equips a
worktree sluice did not make.

### moon, as your CI runs it

With `runner = "moon"`, sluice runs moon in its CI mode, as `moon ci`
does, with three differences:

- **The targets.** sluice runs the branch's `required` tasks that the
  combination affects, with everything that depends on them, where
  `moon ci` runs every affected task.
- **Base and head.** sluice sets `MOON_BASE` and `MOON_HEAD` to the
  branch's tested revision and the combination, so moon never has to
  guess them from a CI provider.
- **No sharding.** sluice splits work by system and host, never with
  `--job`.

Things moon's CI mode does that sluice keeps:

- **`runInCI: false`.** A task marked `runInCI: false` or `runInCI:
  skip` is not run, as in moon's own CI. A required task marked so never
  runs, so it reads missing and the branch can't pass while it is
  required. `sluice submit` refuses a submission to such a branch before
  queuing it, naming the task, the tag it's required through, and the
  line of `.config/sluice.toml` that requires it. Whether a task runs in
  CI is read from the submitting worktree, so the repair lands as any
  submission does: one that lets the task run in CI is taken.
  `sluice status` warns of such a task when a submission ahead changed
  `moon.yml` after this one was taken.
- **The cache, by the branch's rules.** moon's cache counts only as the
  branch's `trust` allows, and a replay is never counted as a run.

From moon's run report, sluice keeps each task's time and tries and the
run's time and estimated savings with the results. The pull request's
report shows them, and no result is read from them. sluice doesn't post
moon's own report: it describes one moon run, while sluice's describes
the combination that lands, every pull request in it, and each test on
each system. A repository can still run moon and its reporting in its
own CI workflow beside sluice.

A combination whose tests could not run is tested again and blames
nobody. After three runs in a row in which its tests could not run, its
newest submission is blamed, and each test is named with why. A command
given to `sluice exec` after `--` is not held to `not_run`.

On a branch whose state is on its remote, a test that passed counts only
where moon's manifest of it is in the store moon keeps it in. sluice
reads the manifest in the store moon's settings give the run and, where
that store lacks it, in the checkout's own `.moon/cache`, which only that
run writes. The same holds for the manifest of each task it depends on.
A test that passed in a run that kept no such manifest in either store
has no result and blames nobody. Its combination is tested again, and
the run counts as one whose tests could not run. Each such test is named
with the key of the manifest that's missing, its own or one it depends
on. sluice also says what the store shows of itself. Either the store
doesn't exist, or the user sluice runs as can't write it or a directory
of it. Otherwise sluice says whether moon kept others of the run's
manifests there. A test whose key
the combination's tree doesn't bear out fails under that key, and its
submission is blamed with why: its manifest names a file the tree
doesn't hold, for one. While a combination is tested again for any of
these reasons,
`sluice status` and `sluice wait` say which host's run had no outcome
and why. The branch's log names the host and the store once, as the
[state page](/sluice/reference/state/#the-log) shows.

On a host whose system Nix manages, NixOS or nix-darwin, sluice notes
the system `/run/current-system` links to as a run begins. A test that
did not pass in a run during which the host's system was switched has no
result and blames nobody, whatever the branch asks of the host. Its
combination is tested again in a new checkout, once the old one's
release has run, and the run counts as one whose tests could not run.
`sluice exec` lists such a test as could not run, naming the system
before and after. On any other host, nothing of its system is read.

## `[checkout]`: where a combination is tested

sluice tests each combination in a clean checkout of its own, apart from
every agent's worktree: `git worktree add --detach` at the combination,
`git worktree remove` when its result is read. What runs in it is
sluice's [equipping](#equipping-a-checkout), the toolchain and, under
moon, `proto install`, `moon --version` and `moon setup`, and then the
tests. No git or worktrunk hooks run there. Whatever a test needs before
it runs goes in the toolchain or in a task the test depends on.

There is one way to make a checkout, so no key names it. An older
`method = "git-worktree"` is still read; any other `method` is refused
with the reason ([ADR 0013](/sluice/decisions/0013/)).

A checkout holds its combination and nothing restored from another
checkout, and saves nothing when it is released. What its tests build is
shared through the host's [`[[caches]]`](#caches), which every run on
the host is given. A policy whose `[checkout]` still declares a
`[[checkout.cache]]` is refused when sluice reads it, saying what
replaced it ([ADR 0049](/sluice/decisions/0049/)): remove it from
`.config/sluice.toml`, and have the host declare the caches its builds
share as [`[[caches]]`](#caches). The caches an earlier sluice saved in
`.git/sluice/.caches/` are removed when the next checkout is made.

| key | values | meaning |
|---|---|---|
| `ports` | e.g. `["web", "db"]` | A port for each name, held for the checkout on this host and checked free, given to the tests as `SLUICE_PORT_WEB`, `SLUICE_PORT_DB` ([ADR 0016](/sluice/decisions/0016/)). An agent's worktree gets its ports on its first `sluice exec` and keeps them until `sluice release`. A preview gets a name for each as well, `SLUICE_NAME_WEB`. |
| `ports_from` | a port, default `20000` | Where a checkout's ports are taken from: the lowest run of as many ports as `ports` names from here in which no port is held by another checkout on the host and each is free. A checkout keeps the ports it was given; a change here counts for checkouts made after it. |

## `[executor]`: what the tests may touch

| key | values | meaning |
|---|---|---|
| `kind` | `"open"` | The tests run as a process group, with nothing held back. |
| | `"sandbox"` | The tests may write only their checkout, their scratch directory, the host's declared [cache directories](#caches) and where the task runner keeps what it shares for the run. They may reach the host's declared cache sockets whatever `network` says (macOS: Seatbelt; Linux: not built yet). |
| | `"container"` | The tests run in a container from the image the policy names. Planned, for a box that runs no agents ([executors and systems](/sluice/guides/executors-and-systems/)); not built. |
| `network` | `false`, `"localhost"`, `true` | No network; only 127.0.0.1 and ::1, for tests that start a server and talk to it; or any. `"localhost"` refuses a name lookup too, so a test that needs `localhost` by name reaches it and one that looks up any other name is refused. |
| `memory` | e.g. `"8G"` | Refused where the executor cannot hold it, never run without it. |
| `docker` | `true` | The tests may reach the host's container engine. A container may mount any path, so such a run is never counted as confined. The engine is looked for where the tests would reach it: at `DOCKER_HOST`, a unix socket or a TCP address, and at the usual sockets. A TCP address is asked plainly, or over TLS as `DOCKER_TLS_VERIFY` and `DOCKER_CERT_PATH` say, as the engine's own client does, and the sandbox lets the tests reach it whatever `network` says. An `ssh://` address and Docker contexts are not read. A host where no container engine answers runs none of the branch's tests. It leaves them for a host that can run them, and `sluice status` says what they wait for. A test that did not pass, after which the engine no longer answers, has no result, and the combination is tested again. A container sees a folder under [`SLUICE_TMP`](#environment) only where the engine's machine shares `/tmp` (`/private/tmp` on macOS) with the host. An engine whose machine shares only the user's home mounts an empty folder in its place. |

Every result records which executor ran it, what it held, and the
executor's grant: what it let the run reach. The sandbox's grant is the
profile it held the run to, with each path named for what it is: the
checkout, its scratch directory, a cache. Checkouts and hosts given the
same profile share one grant. The open executor's is the same for every
run. Results under two grants are never weighed together: a test
that failed under one and passed under another isn't flaky, and a pass
under one proves no replay under another.

A test that did not pass in a run the sandbox refused something has no
result. sluice reads what the sandbox refused from the sandbox's own
report, kept as the run goes, never from what the test printed or how it
failed. The test is named with what was refused, nothing is recorded
under its key, nobody is blamed, and the combination is tested again; a
test that passed keeps its pass. After three such runs in a row the
combination is not passed, each test naming what was refused. A test
that fails in a run the sandbox refused nothing is recorded and blamed
as ever. When sluice can't read what the sandbox refused a run, the run
is read as refused nothing and says so:
`sluice could not read what its sandbox refused the run: <why>`. The cure
for a refusal is the policy, the host's [`[[caches]]`](#caches) or the
test, since nothing was recorded to set aside.

Some executors cannot hold every limit: the open executor holds no `memory`
limit and cannot keep a run off the network, and the sandbox on macOS
holds no `memory` limit. A host whose
executor cannot hold the policy's limits runs none of the branch's tests
and makes no checkout for them. It leaves them for a host that can
([ADR 0012](/sluice/decisions/0012/)). `sluice status` and `sluice wait`
say a combination is waiting for a host that can run its tests as its
policy asks, naming each host that turned it down and why. Where no host
takes it within the branch's [`timeout`](#timeout), the combination is
not passed, and each test says why.

## `[preview]`: a running copy of a landed revision

What brings a [preview](/sluice/guides/previews/) up. Read from the
revision being previewed, since it describes that revision's stack;
what its command may use and reach, `[executor]` and the time limit,
comes from the branch's current revision. With no `[preview]` table,
`sluice preview` refuses the revision.

| key | values | meaning |
|---|---|---|
| `command` | a command | Brings the stack up in the preview's checkout, hands what must stay running to the tools that keep it (a container engine, a process manager), and exits; exit 0 is `up`. A list, the command first, or a string run by `sh -c`. It runs once, under the executor, in a checkout equipped as a combination's is, and ends with its process group. |
| `addresses` | e.g. `{ web = "https://{name.web}.localhost" }` | What a person opens, by label. `{name.<port>}` is the preview's name for a port of `[checkout] ports`, `{port.<port>}` its number. A port not listed is refused. sluice fills them in and never contacts them. |
| `timeout` | e.g. `"10m"` | How long the command may run; the branch's `timeout` when unset. |

```toml
[checkout]
ports = ["web", "api"]

[preview]
command = "moon run app:preview"
addresses = { web = "https://{name.web}.localhost", api = "https://{name.api}.localhost/docs" }
```

`[tests] release` is a preview's release command too: it runs when the
preview is released and removes what the command handed off, routes
included. Each name is one DNS label of at most 63 characters,
`<port>-<revision>--<branch>--<repository>`, cut with a hash when it is
longer, unique among the repository's previews.

## `[landing]`

| key | meaning |
|---|---|
| `subject` | The first line of a landing commit: `{id}` the submission, `{from}` what lands, `{agent}`, `{to}` the branch landed on. `{from}` is the submit's `--subject`, or the title of the pull request it was taken from. With neither, it is the agent's branch, which the commit's `Sluice-From` trailer names either way. The default is the conventional `chore(land): #{id} {from}`. |

## `[systems]`: what a test runs on

The systems a branch can require a test to pass on, each a name for an
operating system and a processor architecture as the toolchain names
them (`linux` or `macos`; `x86_64` or `aarch64`). A result records the
system it ran on, and a pass on one system never stands for another.

```toml
[systems]
linux-x64   = { os = "linux", arch = "x86_64" }
linux-arm64 = { os = "linux", arch = "aarch64" }
macos-arm64 = { os = "macos", arch = "aarch64" }
```

Every other key that names a system, a branch's `systems` and
`systems_for` and a signer's `systems`, uses these
names, and sluice refuses a name the table lacks.

## `[signers."<name>"]`: who may sign results

A policy with signers keeps the state of every branch with a `remote`
on that remote, so every host that submits to it reads and writes the
same queue ([Executors and systems](/sluice/guides/executors-and-systems/)).
A result counts there only when a signer the branch trusts signed it.
A policy with no `[signers]` keeps every branch's state on the host,
as before.

```toml
[signers.box]
ssh = "exec@build-1 ssh-ed25519 AAAA..."
confined = true
systems = ["linux-x64"]

[signers.sessions]
ssh = ".config/sluice.sessions"
```

| key | meaning |
|---|---|
| `ssh` | An SSH public key in `allowed_signers` form, or the path in the repository of a file of them, one per host. A host signs with the private half its own [`[signing]`](#signing) names. |
| `confined` | Whether results it signs may count as confined: no agent could reach the checkout or what the test wrote. Say it only of a host that runs no agents; sluice never works it out. The default is `false`. |
| `systems` | The systems its results may say they ran on. The default is every system. |

A signer has exactly one `ssh`.

### `ssh`: one key, or a file of them

`ssh` takes one of two forms:

- **A line**, for a signer with one host: `ssh = "build-1 ssh-ed25519
  AAAA..."`.
- **A path** in the repository, for a signer with many hosts:
  `ssh = ".config/sluice.sessions"`. sluice reads the file from the
  branch's current revision, as it reads the policy, so adding a host
  is a landing that adds its line, and a submission can't trust its own
  key. A path the revision doesn't hold stops the branch with an error
  naming it.

Either way, each line is in `ssh-keygen`'s allowed-signers form:

```text
# .config/sluice.sessions: one line per session host
laptop ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... a developer's laptop
mini   ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA...
```

The first word is a label: sluice shows it and never checks it. Then
the key type and key, and an optional comment. Blank lines and `#`
lines are skipped. Options between the label and the key, such as
`namespaces=` or `valid-after=`, are skipped too, and restrict nothing:
remove a key's line to stop trusting it. Any key on the list signs as
the signer. A host prints its own line with
`echo "$(hostname -s) $(cat ~/.config/sluice/signing.pub)"`.

## `[branches."<pattern>"]`: rules per target branch

One table per pattern of branch names. Every branch that matches one is
protected: only sluice moves it.

### `required`

The moon targets that must pass for a submission to land on this branch,
run only where the combination affects them. A repository that tags
every test `suite` requires `#suite`; a slower tag such as `#live` can be
required on `main` and not on feature branches. With `runner = "command"` the
command is the only test and `required` is left out.

With `runner = "moon"`, a branch with no `required`, or one whose tags no
task carries, runs no tests: every submission to it lands untested. The
[Claude Code plugin](/sluice/guides/claude-code/)'s setup skill warns of
such a branch when it reads the policy back.

### `parallel`

How many queued submissions are tested at the same time.

Say three agents submit A, B and C to `feature/x`, in that order. sluice
tests three combinations: `feature/x + A`, `feature/x + A + B`, and
`feature/x + A + B + C`. With `parallel = 4`, all three are tested at once,
on the bet that they will pass. If they do, A, B and C land one after
another with no waiting. If `feature/x + A + B` fails, B is blamed, A lands,
and C is tested again as `feature/x + A + C`. The higher the number, the
faster a queue drains when things pass, and the more work is thrown away
when something fails. After a failure sluice tests fewer at once, and
builds back up to this number as things pass. With `batch = true`,
`parallel` is the largest batch instead. (GitHub's merge queue calls
this "maximum pull requests to build"; Mergify calls it "speculative
checks".) [Parallel and batch](/sluice/guides/parallel-and-batch/) shows
both over time and says which to choose.

### `batch`

Whether to test every combination or only the longest.

| value | sluice tests | good for |
|---|---|---|
| `false` | every combination: `+A`, `+A+B`, `+A+B+C`, up to `parallel` at once | fast tests, and a host with room for several runs; each submission lands about one run after it was submitted, and a failure names its submission at once |
| `true` | only the longest, `+A+B+C`, one run at a time; if it passes, all three land together; if it fails, the shorter ones are tested to find which submission broke it | slow tests, where running fewer combinations matters more than landing or blaming quickly |

With `true`, the branch tests nothing else while a batch is under test,
unless it has a [`hold`](#hold). Submissions that arrive meanwhile wait,
and the next batch is whatever queued up: no timer collects one. A branch can't test several batches
at once; several branches can, sharing the host's
[`[runs] max`](#runs).

### `hold`

How long a combination that passed may wait to land while a longer one
holding it is still under test, for example `"5m"`. Unset or `"0s"`,
nothing waits: what passed lands at once.

Say A is under test when B and C arrive ten seconds later. Without a
hold, A lands alone when it passes and B and C land in a second move of
the branch. With `hold = "5m"`, A waits for `+A+B+C`; when that passes,
all three land in one move. If it fails, A lands at once with whatever
else passed ahead of the failure. A never waits longer than the hold
from its own pass. Set it on a branch whose every move starts a build or
a deployment, such as `main`, to how far apart submissions may arrive
and still land together. With `batch = true`, a branch with a hold also
tests the longest combination beside one already under test, so B and
C are not left waiting behind A. `sluice status` names what a held
submission waits for and when its hold runs out.

### `flaky`

A test that has both passed and failed on identical code. `"fail"`
treats it as failing, so the submission is blamed; `"pass"` lets it
through.

Under `"fail"`, a submit names such a test `failed: <test> (flaky)`. It
says whether the test passed or failed in this run, when and on which
combination the same inputs did otherwise, and where that run's log is
kept. The same inputs fail whenever they're tested. When the fail was
the host's or a time limit's, not the test's, the operator sets it aside
with [`sluice set-aside`](/sluice/reference/commands/#sluice-set-aside),
the same inputs read passed from their passes, and the same head lands.
When the test itself is flaky, a fix to it does. A change to what the
test reads made only to have it tested again leaves the fail under its
key for any tree that returns to those inputs.

A test's results are weighed together only under one
[grant](#executor-what-the-tests-may-touch). A result recorded before
sluice recorded grants is weighed only with others recorded before it,
so each test runs once more under its first grant before a replay of it
counts.

### `timeout`

How long one combination's tests may run, for example `"20m"`; an hour
when unset. It times the tests' command, from its start: sluice makes
and [equips](#equipping-a-checkout) the checkout before it. A run that
hits it is ended with everything it started, recorded as timed out, and
its submission is blamed. Tests that ended before it keep their results. A
test still running has none, so nothing is recorded under its key: on a
host that was slow, the same head submitted again is tested afresh.

### `release_retries`

How many more times a combination's checkout whose release failed is
tried, one try a round, before sluice stops and says it is held; `3`
when unset, so four tries in all, and `0` holds it after the first.
Read from the branch's current revision, as the rest of its rules are.
A held checkout never holds the branch: the queue goes on in other
checkouts. `sluice status <branch>` lists it, and `sluice release
sluice/try/<branch>/<n>` runs its release again or, with
`--accept-failure`, frees it without
([commands](/sluice/reference/commands/#checkouts-held-because-their-release-failed)).
A preview's release is tried the same number of times.

### `trust`

Which results satisfy this branch.

On a branch whose state is on its remote, in a policy with `[signers]`,
`trust` lists signer names: a result counts only when one of them signed
it. Without `trust`, such a branch trusts no signer, and its policy's
author names them.

On a branch whose state is on this host, `trust` lists `"confined"` (no
agent could reach the run) and executors (`"open"`, `"sandbox"`,
`"container"`), and the results sluice starts here count unsigned. A
branch without `trust` accepts every run sluice starts
([ADR 0015](/sluice/decisions/0015/)); one whose executor can never
satisfy it starts nothing and says why.

A result moon replays from its cache counts only when a run the branch
trusts recorded a pass for the same inputs on the same system. When
none has, the test runs again without the cache. `"cache"`, which once
let any replay count, is ignored, and `sluice status` warns of it.

### `confined`

On a branch whose state is on its remote, `confined = true` counts only
results whose signer has `confined = true`. On a branch whose state is
on this host, list `"confined"` in `trust` instead; sluice refuses
`confined` there.

### `systems`

The systems every required test must pass on, by the names of
[`[systems]`](#systems-what-a-test-runs-on). Each system is one more run
per combination. Without `systems`, a test takes one pass from any
system.

### `[branches."<pattern>".systems_for]`

The systems one test must pass on, in place of `systems`:

```toml
[branches.main]
systems = ["linux-x64"]

[branches.main.systems_for]
"cli:build" = ["linux-x64", "linux-arm64", "macos-arm64"]
```

A combination of `main` then passes only when `cli:build` passed on all
three and every other required test passed on `linux-x64`.
[Parallel and batch](/sluice/guides/parallel-and-batch/#systems-multiply-runs)
counts the runs this costs.

### `remote`

The git remote the branch lives on, such as `"origin"`. Every round
fetches the branch from it and tests on what the remote holds. A
landing is a push that the remote takes only if its branch has not moved
since the combination was formed. The local branch is then moved to
match. A push the remote refuses because someone else pushed is
reported, and the next round tests again on the new revision. Only the
branch is pushed, and the state stays on the host, unless the policy
has [`[signers]`](#signersname-who-may-sign-results): then the state is
on the remote too, under `refs/sluice/`. A branch without
`remote` is a local branch, and a `remote` the repository has no remote
of that name for lands nothing and says so. A branch must be on its
remote before anything is submitted to it: `sluice branch create
<branch> --from main` makes one there, such as a feature branch, and a submission
to a branch the remote lacks is refused saying so. A host with no local
copy of the branch, only a fetched `origin/<branch>`, reads `remote` from
the policy there. The guide [sluice on GitHub](/sluice/guides/github/)
sets one up.

### `[branches."<pattern>".pull_requests]`

Turns pull requests on for the branch: work reaches it as pull requests
on its `remote`. A pull request joins the queue only once it is out of
draft and meets these conditions on its latest commit.

```toml
[branches.main]
remote = "origin"

[branches.main.pull_requests]
reviews = 1                  # approving reviews on the current head
checks = ["qa/approved"]     # checks other systems post, by name
log_lines = 40               # post the end of a failed test's output
```

| key | meaning |
|---|---|
| `reviews` | How many approving reviews the pull request needs on its latest commit; a review given on an earlier commit doesn't count. The default is 0. |
| `checks` | Checks other systems post on the pull request's latest commit, by the name GitHub shows, each of which must have passed. The default is none. |
| `log_lines` | How many of the last lines of a failed test's output the pull request's report posts, scrubbed of what looks like a secret. At least 1. Without it, the report says where the output is kept on the host instead. |

The conditions are read from the branch itself, so a pull request that
changes `.config/sluice.toml` can't loosen its own. They decide when
sluice takes a pull request, never what it tests: that is `required`.
The table needs `remote`, and sluice refuses a policy that has it
without one. Without the table, `sluice submit` puts work straight into
the queue. The guide [Pull requests](/sluice/guides/pull-requests/) sets
it up.

No key covers operator approval: that is the caller's business, and
a tool submits only once the work is approved.

## The host's config: `~/.config/sluice/config.toml`

How many runs a host tests at once, who sluice pushes as, what it signs
its results as and which caches every run shares are the host's own
settings, never a repository's. So is where its telemetry goes. They live in one file on the host,
`$XDG_CONFIG_HOME/sluice/config.toml`, which is
`~/.config/sluice/config.toml` where `XDG_CONFIG_HOME` is unset, and
every repository on the host reads the same file. sluice never writes
it. A command reads it as it starts, so a change counts from the next
command. Without the file, every setting has its default.

A `sluice run` you keep running looks at the file between its passes.
It takes a change up before its next pass, and says on stderr which
tables it took up. A run already testing keeps what it started with:
its cores, its caches and what its executor lets it touch. A new bound
holds the runs that start after it. A file that is gone, doesn't read,
or names a key that doesn't read leaves the `sluice run` on the
settings it holds. It says so once, with the file and the reason, and
reads the file again once it changes again. To go back to the
defaults, write an empty file. A key changed in place, at the path the
file names, counts once the file next changes or sluice next starts.

A `sluice run` can't swap its telemetry in place, so it takes a change
of `[telemetry]` up by starting itself afresh, in the same process,
with nothing to restart. Whatever runs it, a terminal, a pane or a
service manager, keeps following it, and runs already testing keep
what they started with. Its stderr says the tables it took up, as for
any change. A change it couldn't start afresh for leaves it on the
settings it holds, and it says so once, with the file and the reason.

```toml
[runs]
max = 2        # default: a quarter of the cores; every repository shares it
cores = 12     # default: what the host gives sluice; the cores sluice may use
reserved = 1   # default: 0; runs of the bound agents' own runs never use

[forge]
app_id = 123456                  # the GitHub App sluice pushes as
key = "~/.config/sluice/app.pem" # its private key on this host, or in its place
# credential = ["/opt/forge/bin/token-helper", "--app", "sluice"]  # a command that gives its token
discovery = "60s"                # default: 60s; how often it asks for pull requests

[signing]
signer = "box"                   # the policy's name for this host's key
key = "~/.config/sluice/signing" # an SSH private key on this host

[peers]
lease = "5m"   # how long a quiet host keeps a branch
alive = "2m"   # how often this host shows it is alive

[deliveries]
expire_after = "168h"  # default: 168h; how long an outcome is owed before it shows expired

[[caches]]                       # a cache every run on the host shares
dir = "~/.cache/sccache"

[telemetry]
send = true                          # default: false; whether sluice sends its telemetry
collector = "http://localhost:4318"  # a receiver on the host; needed where send = true

[telemetry.attributes]
"deployment.environment" = "prod"
```

| table | what it sets | a host needs it when |
|---|---|---|
| [`[runs]`](#runs) | How many runs share the host's cores at once, how many of its cores they use, and how many runs are kept for submissions. | Always; each key has a default. |
| [`[runs] systems`](#runs-systems) | The systems the host runs. | It runs only some of the systems a policy names. |
| [`[forge]`](#forge) | The GitHub App sluice pushes as, its key or a command that gives its token, and how often sluice asks GitHub for a branch's pull requests. | It lands on a branch whose remote is on GitHub. |
| [`[signing]`](#signing) | The signer the host signs its results as, and its key. | It runs checks for a branch whose state is on its remote. |
| [`[peers]`](#peers) | How long a quiet host keeps a branch, how often this host shows it is alive, and its name among the hosts. | It shares a branch's remote with other hosts. |
| [`[deliveries]`](#deliveries) | How long an outcome is owed to its worktree before it shows as expired. | Its agents confirm their outcomes sooner or later than in a week. |
| [`[[caches]]`](#caches) | The caches every run on the host shares. | Its builds share a cache, a compile server's or the task runner's. |
| [`[telemetry]`](#telemetry) | Whether sluice sends its telemetry, where, as what, which signals and how much of a failure. | It sends sluice's telemetry. |

### Who writes it

Whoever administers the host writes the file, or whatever manages the
host's configuration does. A host is set up one of three ways.

**A host you administer yourself.** You write the file. Make its
directory, and add the tables the host needs:

```sh
mkdir -p ~/.config/sluice
$EDITOR ~/.config/sluice/config.toml
```

Keep the keys it names, `[forge] key` and `[signing] key`, on the host
with mode `0600`. A host that hands sluice the app's token by a command,
`[forge] credential`, keeps no key of the app's on the host.

**A host under configuration management.** When Nix, Ansible, an MDM or
another tool manages the host's configuration, the file is that tool's.
It is read-only, or a link into the tool's own store, or the tool writes
it again each time it applies the configuration. Change it where the
host's configuration is managed, and apply it from there: an edit on the
host is refused, or lost at the next apply. Nothing need restart sluice
when the managed file changes: a `sluice run` kept running takes the
change up before its next pass. A `->` in the listing marks
a link, and a mode with no `w` marks a read-only file:

```sh
ls -ld ~/.config/sluice ~/.config/sluice/config.toml
```

A key file stays out of any store everyone on the host may read, as
Nix's store is: sluice refuses a key anyone but its owner may read. Put
the key on the host by the tool's own way of handling secrets, or by
hand with mode `0600`, and let the managed file name its path. The
caches' `env` reaches every run exactly as written, and the agents'
shells need the same values: set both from the same managed
configuration, so they agree.

On nix-darwin and NixOS, sluice's own flake gives a module that writes
the file, `darwinModules.sluice` or `nixosModules.sluice`:

```nix
imports = [ inputs.sluice.darwinModules.sluice ]; # nixosModules.sluice on NixOS
services.sluice = {
  enable = true;
  # user = "alice";  # on NixOS: the user who runs sluice
  settings = {
    runs = { max = 3; cores = 12; };
    forge = { app_id = 123456; key = "~/.config/sluice/app.pem"; };
    caches = [ { dir = "~/.cache/sccache"; } ];
  };
};
```

`services.sluice.settings` holds the tables below, each key named as the
file names it. A key you leave out stays out of the file, so sluice's
default holds. A key the module does not know goes into the file as you
wrote it, and sluice refuses it if sluice does not know it either. Name
each key file by its path on the host, from `/` or under `~/`.
Evaluation fails on a key under the Nix store, on a key's text, and on
a Nix path such as `./app.pem`, which would copy the key into the store.
`forge.credential` takes the command that gives the app's token, in
place of `forge.key`, as a list of strings: its program, then each
argument, such as
`[ "/opt/forge/bin/token-helper" "--app" "sluice" ]`. A string in its
place fails evaluation, and so does an argument that holds a GitHub
token's text, since the file is in the store.
The module links `~/.config/sluice/config.toml` to the file it writes,
for nix-darwin's primary user, or on NixOS for `services.sluice.user`.
A file already there that is no link moves once, to
`config.toml.before-nix`. The module puts sluice in the system profile:
set `services.sluice.package` to install your own build, or to `null`
to install none. It writes nothing else and installs no service, since
sluice needs none. A configuration may import the module twice, itself
and through another flake's module that imports it: it declares
`services.sluice` once.

**Several hosts as peers.** Hosts that submit to one branch on a remote,
or run its checks, each have a file of their own, set up on each host in
one of the two ways above. Each holds what is that host's alone: its own
signer and key in `[signing]`, the systems it runs in `[runs] systems`,
its own bound in `[runs]`, and `[peers] name` where two hosts share a
name. Each host that lands a branch on GitHub names the app in its own
`[forge]`, with the app's key on that host. What the hosts share is the
branch's policy, whose [`[signers]`](#signersname-who-may-sign-results)
list each host's public key. The guide
[Executors and systems](/sluice/guides/executors-and-systems/) sets them
up.

### `[runs]`

`max` is the host's bound: how many runs share the host's cores at once,
whatever repository or branch each is for. It counts submissions' runs,
the runs agents start in their own worktrees with `sluice exec` and
previews' commands together, across every repository on the host. Every
sluice on the host reads that one count from the host's state
([ADR 0047](/sluice/decisions/0047/)), and a run waits while the bound is
full. sluice reads nothing of how busy the host is: what runs beside it,
the agents' sessions among it, is yours to leave room for when you set
`max` and `cores`.

`cores` is how many of the host's logical CPUs sluice may use. Without
it, sluice uses what the host gives it. On Linux that is the process's
CPU affinity or its cgroup's quota, as systemd and container engines
set, and every core where the host sets neither. Each run gets the
cores sluice may use divided by `max`, and never less than one: sluice
hands that number to the run as `SLUICE_JOBS`, and to moon as
`--concurrency`. With the bound shared, the runs going at once use the
cores sluice may use and no more. With no `max`, the bound is a quarter
of those cores, and never less than one, so a lower `cores` lowers the
default bound too; set `max` to keep it where you want it.

A Mac never limits what sluice may use, so there the runs going at once
take every core. Set `cores` to leave the rest for the agents' sessions
and whatever else runs beside sluice: on a Mac of eighteen cores,
`max = 3` and `cores = 12` run three at once with four cores each and
leave six. sluice refuses to start, with the setting named, when
`cores` is below 1, when it is more than the host's cores, or when the
host already limits sluice by its affinity or quota. That limit is then
what sluice may use, so leave `cores` out.

`max` is the one setting for memory as well. Nothing but moon keeps to
the share of cores, and an executor's `memory` limit holds the run's own
processes, never the containers they ask a container engine for. A
repository whose tests bring up a stack of containers sets `max` to how
many of those stacks the host holds in memory at once, whatever its
count of cores.

`reserved` splits the bound. It keeps that many runs of the bound for
submissions: an agent's run in its worktree waits rather than take one,
and a submission's run starts whenever the bound has room. With the
default, 0, agents' runs may hold the whole bound. A `reserved` at or
above `max` leaves agents no run, and sluice refuses to start with the
setting named. `sluice overview --json` shows the bound, what holds it
and the reserve as `room`, and its counts line says when agents' runs
hold back a submission that would otherwise start. What the host records
of its runs and ports is on the [state page](/sluice/reference/state/).

### `[forge]`

The app sluice pushes as, for every branch whose `remote` is on GitHub.
sluice makes a token from the key for each repository when it needs
one, keeps it in memory, and gives it only to its own fetches and
pushes. An agent's own git gets nothing from it, so a ruleset whose only
bypass is the app refuses an agent's push to the branch. `key` is a
path on the host, with `~` for the home directory; sluice refuses a key
file that anyone but its owner may read, and names the mode it needs,
`0600`. Without `[forge]`, sluice pushes with the host's own git
credentials, and `sluice status` says the branch has no identity of its
own. The guide [Give sluice its own GitHub
identity](/sluice/guides/github-app/) sets it up, and
[`sluice forge check`](/sluice/reference/commands/#sluice-forge-check)
says whether it is set up right.

`credential` names a command that gives the app's token, in place of
`key`, for a host whose own tool keeps the app's key. A table names
`key` or `credential`, never both, and sluice refuses one that names
both or neither. Write the command as a list: its program, then each
argument.

```toml
[forge]
app_id = 123456
credential = ["/opt/forge/bin/token-helper", "--app", "sluice"]
```

sluice runs the command with no shell, so a string in its place is
refused. Name the program by its full path, under `~/`, or by a name
the `PATH` finds. sluice asks the command as git asks a credential
helper. It runs the command with `get` after its arguments, and writes
`protocol=https`, `host=github.com` and `path=<owner>/<repository>` on
its standard input. It reads `password`, the token, and
`password_expiry_utc`, the token's end in seconds since 1970, from its
standard output, and passes over every other line. sluice holds the
token in memory for that repository alone, and asks again five minutes
before its end. A token given with no end is held until GitHub refuses
it.

When GitHub refuses the command's token, sluice runs the command with
`erase` and the refused token, then with `get` for a fresh one. It
never tries the refused token again, and a `get` that gives it back is
no fresh token. A command that exits non-zero, prints no `password`, or
gives nothing in thirty seconds fails the call that needed the token.
The failure names the command and gives what it wrote on its standard
error, with anything shaped as a GitHub token replaced by `[redacted]`.
Where the command wrote more than 64 KiB there, the failure gives its
whole lines within the first 64 KiB. sluice asks GitHub once, with each
token the command gives, which repositories it reaches. It refuses a
token that doesn't reach the repository, and names what it reaches in a
refusal's report. `sluice forge check` with a command checks that the command
gives a token, that the token is the app's that `app_id` names, and that
it may push. GitHub shows a token's other grants to nobody holding only
the token, so check the app's other permissions where its key is kept.

`discovery` is how often sluice asks GitHub for the open pull requests
into a branch that takes them while nothing on the host changes, a
duration such as `"60s"` or `"2m"`. The default is `"60s"` and the
least is `"10s"`: sluice refuses a shorter one, naming `discovery`. A
submit on the host, a landing, or a change to the branch's queue asks
at once whatever it is set to. What it sets is how soon sluice sees what
happens on GitHub alone: a pull request opened by hand, a review, or
another system's check. A shorter one sees those sooner and spends more
of the app's allowance of calls, which every repository the app is
installed on shares. Between two askings, sluice asks GitHub nothing.

### `[runs] systems`

The systems this host runs, by the policy's
[`[systems]`](#systems-what-a-test-runs-on) names. Without it, the host
runs the one system it is. `sluice run` refuses a system whose operating
system and architecture the host isn't.

### `[signing]`

The signer this host signs its results as, `signer`, one of the
policy's [`[signers]`](#signersname-who-may-sign-results), and the path
of its SSH private key, `key`, whose public half the policy lists.
sluice refuses a key file anyone but its owner may read, as it does
`[forge] key`. A host with no `[signing]` records no result on a branch
whose state is on its remote, and says so on every run. Make the key
with `ssh-keygen -t ed25519 -f ~/.config/sluice/signing -N ""`.

### `[peers]`

How long a branch whose state is on its remote stays with the host
that last wrote it, and how often this host shows it is alive. The same
for every branch the host serves, and never a branch's: a policy that
sets either is refused.

| key | meaning |
|---|---|
| `lease` | How long this host waits, having seen a branch's state stand unchanged, before it takes the branch from a host that has gone quiet. It counts by its own clock, never by the time another host wrote. The default is `"5m"`. |
| `alive` | How often this host rewrites its sign of life. A claim on a run lapses once its host's sign of life has stood unchanged this long, and another host runs it. It must be shorter than `lease`. The default is `"2m"`. |
| `name` | What this host is called among the hosts that share a remote: in a branch's state as its lead, and on the runs it claims. The default is the host's own name; give two instances on one machine a name each. |

### `[deliveries]`

How long an outcome the host's rounds decide is owed to the worktree or
pull request that submitted it before it shows as expired. An outcome
stays owed, and every `sluice inbox` in its worktree hands it over
again, until its agent confirms it with `sluice confirm <key>`. Past
`expire_after` it is still owed: `sluice inbox` says it has expired,
and `sluice status <branch>` lists it under its worktree, for the
operator to confirm with `sluice confirm <key> --as operator` where the
worktree is gone. The span is fixed by the host whose round decides the
outcome.

| key | meaning |
|---|---|
| `expire_after` | A duration such as `"168h"`, `"72h"` or `"90m"`, at least a second. The default is `"168h"`, a week. A value that is no such duration is refused, naming `[deliveries] expire_after`. |

### `[telemetry]`

Whether sluice sends its telemetry, where, as what, which signals and
how much of a failure. sluice sends OTLP over HTTP, protobuf, to a
collector, as the [telemetry reference](/sluice/reference/telemetry/)
describes. A host with no `[telemetry]` sends nothing unless a command
asks. A `sluice run` you keep running takes a change of the table up
by starting itself afresh, in the same process, as
[the host's config](#the-hosts-config-configsluiceconfigtoml) says.

```toml
[telemetry]
send = true
collector = "http://localhost:4318"
logs = "tail"
logs_tail_bytes = 4096

[telemetry.attributes]
"deployment.environment" = "prod"
```

| key | meaning |
|---|---|
| `send` | Whether sluice sends its telemetry, `true` or `false`. The default is `false`: sluice sends nothing. `true` needs a `collector`. |
| `collector` | The collector sluice sends to, an `http://` address: a host, a port and at most a path, such as `"http://localhost:4318"`. sluice sends to its `/v1/traces`, `/v1/metrics` and `/v1/logs`. Name a receiver on the host, which forwards over TLS: sluice sends plain HTTP, and refuses an `https://` address. A table with `send = true` names one. Unset, the table sends nothing, and a command's own `--otel` sends where `OTEL_EXPORTER_OTLP_ENDPOINT` says, else to `http://localhost:4318`. |
| `service` | The service sluice reports as, its `service.name`. The default is `"sluice"`. |
| `traces` | Whether sluice sends traces, `true` or `false`. The default is `true`. |
| `metrics` | Whether sluice sends metrics, `true` or `false`. The default is `true`. |
| `logs` | How much of a failed check's output sluice sends as logs. `"off"` sends no logs at all. `"summary"` sends which task failed and where its output is kept, never the output. `"tail"` sends the output's end, scrubbed of what reads as a secret. The default is `"summary"`. |
| `logs_tail_bytes` | At `logs = "tail"`, the most of a failed check's output sluice sends, in bytes, from 1 to 65536. The default is 8192. |
| `attributes` | More resource attributes on everything sluice sends, each a string, such as `"deployment.environment" = "prod"`. The default is none. |

Everything sluice sends carries `service.name`, `service.version`, the
release, and `host.name`, the host's own name. An `attributes` entry
`host.name` names the host in its place.

sluice refuses to start on a `[telemetry]` it cannot read, naming the
table and the key: a key it does not know, a `collector` that is no
`http://` address or names no host, `send = true` with no `collector`,
or an empty `service`. It refuses a
`logs` other than `off`, `summary` or `tail`, and a `logs_tail_bytes`
below 1 or above 65536. It refuses an attribute with no name, and one
named `service.name` or `service.version`, which `service` and the
release name. A sluice too old to know `[telemetry]` refuses the file, as it
refuses any table it does not know.

A command's own choice holds for that command alone, over the table.
`--otel` or `SLUICE_OTEL=1` sends where `send` is unset or `false`, and
`--otel=false` or `SLUICE_OTEL=0` sends nothing where `send = true`.
`--otel-service-name`, `--otel-logs` and `--otel-logs-limit` choose
`service`, `logs` and `logs_tail_bytes` the same way, as do
`SLUICE_OTEL_SERVICE_NAME`, `SLUICE_OTEL_LOGS` and
`SLUICE_OTEL_LOGS_LIMIT`. sluice holds a limit above 65536 to 65536.

OpenTelemetry's variables only turn telemetry or a signal off, or name
the collector where the table names none:

- `OTEL_SDK_DISABLED=true` turns telemetry off, and
  `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` or
  `OTEL_LOGS_EXPORTER` at `none` turns that signal off. None of them
  turns on a signal the table turns off.
- Where the table names a `collector`, sluice reads none of the caller's
  `OTEL_EXPORTER_OTLP_*`: a session's endpoint, headers and protocol
  never take sluice's telemetry. A table names none only where it does
  not send, and there, for a command's own `--otel`, they name the
  collector, its headers and its protocol.
- Where the host has a `[telemetry]`, sluice reads neither
  `OTEL_RESOURCE_ATTRIBUTES` nor `OTEL_SERVICE_NAME`.

A host with no `[telemetry]` reads the environment as a command is given
it, `OTEL_EXPORTER_OTLP_*`, `OTEL_RESOURCE_ATTRIBUTES` and
`OTEL_SERVICE_NAME` among it.

### `[[caches]]`

The caches the host keeps for every build on it: a compile server's
cache, or the task runner's cache shared by every worktree. Every run
sluice starts on the host is given them, under any executor: a
submission's run, an agent's `sluice exec` and a preview's command
alike. An agent's shell and a run on the host then reach the same
caches.

```toml
[[caches]]                       # a compile server's cache
dir = "~/.cache/sccache"
socket = "~/.cache/sccache/server.sock"
env = { RUSTC_WRAPPER = "sccache", SCCACHE_DIR = "/Users/me/.cache/sccache", SCCACHE_SERVER_UDS = "/Users/me/.cache/sccache/server.sock" }

[[caches]]                       # moon's shared worktree cache
dir = "~/.moon/cache/shared"
env = { MOON_CACHE_SHARED_WORKTREE_CACHE = "true", MOON_EXPERIMENT_CAS_OUTPUTS_CACHE = "true" }
```

| key | meaning |
|---|---|
| `dir` | The directory the cache keeps, outside every checkout: an absolute path, or one under `~/`. A sandboxed run may write it. |
| `socket` | The socket of the cache's server, where it has one, written the same way. A sandboxed run may connect to it whatever the policy's `network` says. |
| `env` | The variables the cache's clients read, each a string. Every run gets them exactly as written: sluice expands nothing in them, so write the same values into the agents' shells. |

A repository's policy declares no cache. The caches are the host's, and
every repository on the host shares them. A run that holds a test the
policy marks [`cold`](#tests-how-tests-run) gets none of them.

sluice starts, ends and locks nothing of a cache, and names no cache's
tool. You run a cache's server, as you run the container engine. When
the server is down, a run goes on without it, and what the run's tools
do then is theirs.

sluice refuses to start when a cache has no `dir`, a key it does not
know, a `dir` or `socket` that is neither absolute nor under `~/`, or an
`env` value that is not a string. It names the cache by its place among
the `[[caches]]`, and the key. It refuses a variable it sets for every
run itself: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`,
`LANG`, `TERM`, `PROTO_HOME`, `MOON_HOME`, `LC_*` and `SLUICE_*`. It
refuses one variable set by two caches.

A shared cache lies outside every checkout, so one checkout never
writes another's. sluice refuses a run whose checkout is a cache's
`dir`, lies inside one or holds one, before anything of the run starts.

Share moon's cache through moon's own shared worktree setting, as
above. moon then shares what it built among every worktree on the host,
and keeps each worktree's run report and task states in the worktree,
where sluice reads them. Where it keeps what it shares is moon's to
say: in the base checkout's `.moon/cache` when the repository has a
base checkout holding `.moon`, otherwise in `cache/shared` under moon's
home, `MOON_HOME` or `~/.moon`. moon's home is the one moon's command
sees in a run, with `[tests] toolchain` sourced: a toolchain that sets
`MOON_HOME` moves it. A sandboxed run may write where moon keeps it,
wherever the repository moves moon's home and whatever `dir` says, and
sluice reads there the manifest by which a host proves a task's key.
The cache's `dir` need not name it.

`MOON_CACHE_DIR` moves nothing of moon's. moon 2.6.1 reads no such
variable: it sets it for the tasks it runs, naming the workspace's own
cache.

## Environment

| variable | set by | meaning |
|---|---|---|
| `SLUICE_AGENT` | the agent | The agent's name, recorded on its submissions and shown in `sluice status`. |
| `SLUICE_CHECKOUT` | sluice, for a run | The absolute path of the checkout under test. |
| `SLUICE_TMP` | sluice, for a run | A scratch directory of the run's own, for sockets, pid files and data directories, so parallel checkouts never collide. It is in a directory of the user's own under `/tmp` ([state](/sluice/reference/state/)). Its path is one length for every checkout of every repository on the host: 45 bytes on macOS for user 501, 38 on Linux for user 1000. A socket a tool keeps a few folders down fits under the 104 bytes macOS allows: `$SLUICE_TMP/x/sessions/work/x-client.sock` is 75. An agent's worktree keeps one across its runs, until the host empties `/tmp`, as it may when it restarts. The next run makes it again, empty, so keep nothing there the tests can't make again. A container sees a folder under it only where the container engine's machine shares `/tmp` with the host, as [`docker`](#executor-what-the-tests-may-touch) says. |
| `SLUICE_PLACE` | sluice, for a run | A short name of the checkout's own, for labelling what the tests start, so the release command finds it. |
| `SLUICE_PORT_<NAME>` | sluice, for a run | The port given for each name in `[checkout] ports`, upper-cased, `-` written `_`. |
| `SLUICE_NAME_<NAME>` | sluice, for a preview | The preview's name for each port: one DNS label, unique among the repository's previews, for a proxy to route to the port. |
| `SLUICE_BRANCH` | sluice, for a preview | The branch the preview was asked for through. |
| `SLUICE_REVISION` | sluice, for a preview | The full revision the preview holds. |
| `SLUICE_JOBS` | sluice, for a run | The cores the run may use: the cores sluice may use, `[runs] cores` where set and else what the host gives it, divided by `[runs] max`, the bound every repository on the host shares, and never less than one. |
| each `[[caches]]` `env` | the host's config, for every run | The variables a cache's clients read, exactly as the host's [`[[caches]]`](#caches) declares them: for a submission's run, an agent's `sluice exec` and a preview's command, under any executor. Nothing else of sluice's own environment reaches a run. |
| `SLUICE_OTEL` | the caller, for one command | `1`, `true`, `yes` or `on` turns telemetry on for the command, and `0`, `false`, `no` or `off` turns it off, over the host's [`[telemetry] send`](#telemetry); see [telemetry](/sluice/reference/telemetry/). |
| `SLUICE_FORGE_API` | tests | Where the `[forge]` app's API calls go instead of `https://api.github.com`: a fake one. |
