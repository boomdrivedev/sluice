---
title: State
description: Where sluice keeps its state, and what a landing leaves in history.
sidebar:
  order: 3
---

Everything sluice decides is written to plain files before it is acted
on, in the repository's git directory, never committed and never pushed,
readable with plain tools whether or not sluice is running.

```
.git/sluice/
  results.jsonl        every test result of the repository, with its executor
  .next-id             the next submission's id
  .lock                held while an id is given or a branch's records are written
  feature/concurrent-units/
    version            57: bumped on every write, so two rounds never both write
    branch.json        the queue's own state, one document
    submissions/14.json
                       a submission still in the queue, or blamed or conflicting
    .finished/12.json  a submission landed or withdrawn, moved here unchanged
                       once nothing still waits on it
    finished.jsonl     one line for each submission in .finished/: its id,
                       state and worktree
    log.jsonl          every submission, test run, result, blame and landing, in order
    deliveries.jsonl   each decided outcome owed to the work that submitted it,
                       its hand-overs and confirmations, in the order added
    lock               held by a round of this branch
  .logs/feature/concurrent-units/<candidate>/<task>.log
                       what each failed test printed, for the last 20 failed combinations
  .refused/feature/concurrent-units.json
                       while this host's rounds can't read the branch's remote:
                       what they couldn't read, since when, for how many rounds,
                       and which of it the log holds yet
  .intake/main.json    while this host can't take in a branch's pull requests:
                       what its intake asked of GitHub and the words it was
                       refused with, since when, for how many intakes, and
                       which of it the log holds yet
  .places/<checkout>.json
                       a checkout's record: its kind, path, branch, the ports
                       it holds on the host, each name and its port, and, for
                       a worktree, when its last run started and how it ended
  .places/<checkout>/  a candidate's checkout, made for its run and removed after
  .previews/<preview>.json
                       a preview's record: its branch, revision, ports, names,
                       addresses, when it was made and its command's run; for a
                       failed preview, what its command printed and nothing held
  .previews/<preview>/ a preview's checkout, kept until it is released
  .tmp/<code>/         the scratch of a checkout an older sluice made, at the
                       code its record names, or at the checkout's name where
                       the record names none. A candidate's or a preview's is
                       removed when it is released. A worktree's moves to the
                       user's directory, with what is in it, at the worktree's
                       next run once none of its runs is going. A directory
                       here that no record names is removed by the next round
                       or sluice exec on the host
  .runs/<checkout>/    a candidate's run, or a preview's command
  .runs/<worktree>/<n>/
                       each run an agent started in its worktree, numbered;
                       release/ beside them is where sluice release runs
  in each run directory:
    request.json       what to run, where, with which environment and limits
    lock               held by the run's supervisor for as long as the run is going
    pgid               the run's process group, so the whole of it can be ended
    out.log            what the run printed
    out.times          when out.log grew: its length and the time, at most once a second
    result.json        how the run ended, once it has
    release.log        what the release command printed
    released           written once the release command has run
    watch.out          under the sandbox, the sandbox's reports of what it
                       refused the run, kept from before the run starts
                       until a second after it ends
    watch.err          what the program that keeps them said
    watch.pgid         that program's process group, ended with the run
    watch-error        written when that program wasn't ready as the run
                       started: what it said and how it ended
```

A checkout's scratch, the [`SLUICE_TMP`](/sluice/reference/configuration/#environment)
its tests may write, is kept apart from every repository, in a directory
of the user's own under `/tmp`. Its path is the same short length for
every checkout of every repository on the host, whatever they are called
and wherever they are:

```
/tmp/sluice-<uid>/     the user's own; /private/tmp/sluice-<uid>/ on macOS
  <repository code>/   a short code made from where the repository's git
                       directory is, one for each repository on the host
    <code>/            a checkout's scratch, SLUICE_TMP, which its record names:
                       a short code made from the checkout's name; a worktree
                       keeps its own across runs
```

sluice makes `sluice-<uid>` and the repository's directory under it
readable, writable and searchable by the user alone. Where either is
already there as a link, a file, another user's directory or one others
may enter, sluice makes no checkout under it and changes nothing of it.
It says the directory's path and what is wrong with it, and makes it
again once you remove it.

The host may empty `/tmp` when it restarts, or clean out what nobody
has touched for days. A checkout's next run then makes its scratch
again, empty, at the path its record names. The next round or `sluice
exec` on the host removes a directory under the repository's code that
no record names, and never looks at another repository's.

What belongs to the host and not to any repository is kept under
`$XDG_STATE_HOME/sluice` (default `~/.local/state/sluice`), shared by
every repository on the host and never pushed:

```
~/.local/state/sluice/
  forge/answers/<h>.json
                       GitHub's last answer at one address, with the ETag
                       and time it came with, so the next read asks GitHub
                       only whether it changed
  forge/pauses/<app>-<owner>-<allowance>.json
                       until when GitHub said the app's allowance of calls
                       is spent, in GitHub's words; the owner, the account
                       the repository is on, names the app's one
                       installation there, whether its token was minted
                       from the app's key or given by a credential command
  forge/requests/<owner>/<repository>/<branch>.json
                       when sluice last asked GitHub for the open pull
                       requests into the branch, and what it read and
                       posted of each, so that one that hasn't changed
                       isn't read again
  forge/requests/<owner>/<repository>/<branch>.lock
                       held while that file is written
  forge/requests/<owner>/<repository>/<branch>.moved
                       touched by a submit that opened, updated or marked
                       ready a pull request into the branch, so the next
                       round asks at once
  forge/follows/<owner>/<repository>/<branch>.json
                       on a host that doesn't keep the branch, which
                       takes its pull requests in, each one it follows:
                       one a submit on the host opened, updated or marked
                       ready, or one a wait or status asked about, with
                       what GitHub last said of it and of sluice's check
                       on its head, and when
  forge/follows/<owner>/<repository>/<branch>.lock
                       held while that file is written
  forge/intake/<owner>/<repository>/<branch>.json
                       the branch's intake mark as this host last saw it on
                       the remote, when it first saw it there, and when it
                       last pushed it
  forge/intake/<owner>/<repository>/<branch>.waits/<process>.hold
                       held by each wait on the host that reads the
                       branch's pull requests, for as long as it runs: the
                       last of them to end removes the mark
  forge/intake/<owner>/<repository>/<branch>.waits/.lock
                       held while a wait's hold is made or let go
  host/lock            held by sluice's host process for as long as it runs
  host/start.lock      held by a command starting the host process, and by
                       the process as it decides to end or hands over
  host/version         the host process's version, pid and when it started
  host/log             what the host process says: its passes, the branches
                       it passed by, its warnings; the process before's kept
                       as host/log.1
  host/repositories    the repositories the host process finds again when it
                       starts: each repository's git directory, listed by the
                       first command of sluice's that found it has sluice
  ports/.lock          held while a port is held or let go
  ports/held/<port>    a port a checkout holds, naming the checkout's record
  readers/<repository>.lock
                       held by sluice's host process while it holds the
                       repository, or by a watch or wait of a sluice from
                       before it
  readers/<repository>.json
                       the overview it last read, as `sluice overview --json`
                       prints it, for every watch and wait to follow
  readers/<repository>.driver
                       held by sluice's host process while it holds the
                       repository
  readers/<repository>.version
                       the version of sluice that holds the lock
  readers/<repository>.lease
                       held by sluice's host process while it takes the
                       reading over from an older sluice's watch or wait
  running/.lock        held while a run is counted against the bound
  running/<run>.json   each run going on the host, every repository's:
                       a submission's or an agent's, its repository, its
                       checkout, where its output is, the process that
                       started it and when
  running/<run>.hold   held by the process that started the run until the
                       run is under way
  sessions/.lock       held while an agent's run's file is written
  sessions/<worktree>.json
                       each run an agent has going in its worktree: the
                       worktree, its branch, its process, when it started,
                       where its output is, the command it runs and how far
                       its tasks have got
```

sluice's host process answers commands on a socket in the user's own
short directory beside the checkouts' scratch, `/tmp/sluice-<uid>/` as
macOS resolves it, named `host-` and ten hex digits of the SHA-1 of the
state folder's resolved path, so each state folder has its own process
and the socket's path stays within the 104 bytes macOS allows. The
process removes a socket a process before it left, and its own as it
ends. One process runs for each user: a second one started while the
first holds `host/lock` ends at once. Removing `host/`, or the whole
state folder, ends the process at its next look.

A checkout's name is written as one file name, each `/` of it as `%2F`
(or `+2F` for a directory a test works in), so a worktree's whole path
is one name. macOS and Linux allow a file name of 255 bytes, so a name
longer than 246 once written is cut short. It keeps its first 160
bytes, then `%~` (or `+~`) and 16 hex digits of the SHA-1 of the whole
name. Each record carries the whole path or revision it is for,
so nothing needs the file name read back.

A preview's record is kept apart from the other checkouts' so that a
sluice older than previews never reads one. A preview's command counts
in `running/` and is listed in `sessions/` while it runs, as an agent's
run is.

Every sluice on the host reads `forge/` before it calls GitHub. An
answer is used only when GitHub replies that it hasn't changed, and one
no read has used for a day is removed. A pause stops every sluice on
the host from calling GitHub until the time GitHub named. A branch's
file in `requests/` says when its pull requests were last asked for, so
a sluice started again asks nothing until `[forge] discovery` has passed
since any sluice on the host asked. A branch's file in `follows/` says
when each pull request in it was last read, so every sluice on the host
reads one at most once in a quarter of `[forge] discovery`. A request
leaves it once a wait or status has said it landed or closed, or once
this host takes it in. Nothing in `forge/` holds a token,
and any file in it may be removed at any time: sluice then asks GitHub
as if it had kept nothing.

A port is free again when the checkout whose record its file names is
gone, and a release lets go of each port its checkout's record names. A
checkout's ports are taken when it is made, or a worktree's at its first
run, and read from its record ever after, whatever its policy lists
later. Files `ports/<n>`, which an earlier sluice held, are read by
nothing and may be removed once no earlier sluice runs on the host.
A file in `running/` counts against the bound while its `.hold` is held
or its run is going. sluice's host process writes one for each run's
room from the count that chose the run to its start, and holds its
`.hold` until the run is under way; a file of a sluice from before the
host process counts the same, so the two count each other. One whose
run has ended, or whose starter stopped before the run began, holds
nothing: the next run that starts removes it. A file in `sessions/` is there while its agent's run is going, and
its `sluice exec` rewrites it as each task ends, for `sluice overview`
to show; nothing that decides a result reads it.

One process on the host reads a repository's queues for every watch and
wait of it: sluice's host process, for every repository it holds. It
holds the repository's `.driver` file's lock and its `readers/` lock,
and writes the overview file beside them after each of its passes,
whether or not anything is queued. A watch or wait that finds no host
process answering starts it, and follows the file; none takes a lock or
a lease, and none reads the queues itself. The host process keeps the
overview in the file, rewritten whole each time it changes, so nothing
reads half of one. A read of the queues it couldn't make goes in the
file in its place, as the error document `sluice overview --json`
prints when it fails. It holds the words the read was refused with, and
the host process reads again at its next pass. Where the repository has
no sluice, the file holds the overview with `"set_up": false`, every
watch ends on it, and the host process holds the repository no longer.
A host process that takes the lock removes that file before it reads, so
nothing that starts once sluice is set up ends on it. A wait takes its
submission's progress from that file, and reads its submission's own
file once, when the submission leaves the queue, to say how it ended.
When the host process stops, however it stops, the lock goes with it,
and every watch and wait starts it again at its next look, a second for
a watch, three for a wait, following the last file until the new one
writes. Where a watch or wait of a sluice from before the host process
holds the lock, the host process takes its lease; the older one lets go
at its next look and follows from then on, and the host process reads
from its own next pass. A sluice from before the lease takes no part:
it names no version and never looks at the lease, so it reads until its
watch or wait ends, and the host process follows it meanwhile. Where an
older `sluice run` holds the `.driver` file's lock, the host process runs
no round of that repository and follows its file until it ends. The file
names take the repository's git directory, which all its worktrees
share, written as one file name as a checkout's is. A status, a
one-shot `sluice overview`, `sluice` with no command and a forecast
read none of these files' locks: each asks the host process for a
reading, starting it where none answers, and reads the state as the
host holds it since, fetching nothing of its own
([one-shot reads](/sluice/reference/commands/#one-shot-reads)).

## A branch whose state is on its remote

A branch with a `remote`, in a policy with
[`[signers]`](/sluice/reference/configuration/#signersname-who-may-sign-results),
keeps its state on that remote as refs. Every host that submits to the
branch reads the same queue, and none of them has to be running for the
rest to go on:

```
refs/sluice/state/<branch>       one commit: the branch's files above, branch.json,
                                 submissions/, .finished/, log.jsonl,
                                 deliveries.jsonl and version, replaced on every write
refs/sluice/runs/<signer>        each signer's signed results, only ever added to:
                                 <combination>/<system>/<n>.json, one run's results
                                 in a signed envelope
refs/sluice/ids                  the next submission's id, for every host
refs/sluice/heads/<commit>       a submitted head, until the submission leaves the queue
refs/sluice/combinations/<branch>/<id>
                                 a combination open to any host with room
refs/sluice/logs/<branch>/<id>   what a failed combination's tests printed, for
                                 the last 20
refs/sluice/hosts/<host>         a host's sign of life and the room it has, rewritten
                                 by that host alone
refs/sluice/set-aside            the results `sluice set-aside` set aside, one file
                                 each, only ever added to
```

A write to a branch's state is a push that the remote takes only if the
ref is still the commit the round read, so the version rule below holds
across hosts. A signer's results are added on top of whatever its ref
holds, so two hosts that sign as one signer never lose each other's.
What only this host keeps, its locks, its runs' tasks and copies of the
logs it kept, is in `.git/sluice-remote/<remote>/`. A result moon
replayed is kept there too: a replay is nobody's observation, so nobody
signs it. Beside them, `refused/<branch>.json` holds what this host's
rounds couldn't read of the remote while that lasts, as `.refused/` does
for a branch whose state is on the host, and `intake/<branch>.json` what
this host couldn't take in of the branch's pull requests on GitHub, as
`.intake/` does.

Read any of it with git, on any host:

```sh
git fetch origin '+refs/sluice/*:refs/sluice-fetched/origin/*'
git show refs/sluice-fetched/origin/state/main:branch.json
git ls-tree -r refs/sluice-fetched/origin/runs/box
```

sluice's host process reads `results.jsonl` once for its life, each
time from where it last read it, and the whole file again only where
it shrank or was replaced. It reads each signer's ref as its last fetch
brought it, checking each signed record's signature once. A command
asks it for the results it needs, by a check's key, a combination or a
task, and reads none of these itself.

No landing, removal of a branch or release of a checkout takes a result
out of `results.jsonl` or a signed record off a signer's ref. A result
belongs to its check's key and its system, not to a branch, so a pass a
removed branch recorded still counts for any branch whose tree gives
that check the same key.

A signed result is an [in-toto](https://in-toto.io) statement in a
[DSSE](https://github.com/secure-systems-lab/dsse) envelope. Each test
and system is one subject, named `<test>@<system>`, its digest the
test's key. The predicate holds each outcome, the branch, the
combination, and what the signer claims of the run: its executor,
whether it was confined, and its system. It also holds what the
executor held the run to, its memory, network and container engine,
and its grant, each left out where it held none. A statement signed
before these has neither, and its results are weighed with no new
run's. A host signs with the SSH key
its [`[signing]`](/sluice/reference/configuration/#signing) names. sluice counts a result
only once the signature checks against a signer the branch's policy
names.

## A branch's own record

`branch.json` holds the queue's own state: the revision its
combinations are formed on, the combinations, a landing under way and
the outcomes still to deliver. Two fields say what a round reads the
branch against:

- `landed`: the revision every later read of the branch must contain.
  A landing sets it to the revision it landed, `sluice branch accept`
  to the revision accepted, and `sluice branch create` to the revision
  it made the branch at. A record an older sluice wrote has none. The
  next round takes the log's last `landed` line, or the revision the
  queue was last formed on when that one lacks the landing, as for a
  branch deleted and made again.
- `held`: while the revision the branch was last read at lacks
  `landed`, `{"read": "10e4791a2b", "lacks": "c43609b8e1", "since":
  "2026-10-23T09:16:40Z"}`. Nothing is formed or landed on the branch
  meanwhile. It goes on the first round that reads a revision holding
  `landed`, or with `sluice branch accept`.
- `copies`: for a branch kept on a remote, each host whose own branch
  of that name a round could not set to `landed`, by the host's name:
  `{"build-2": {"at": "10e4791a2b", "lacks": "c43609b8e1", "why": "git
  holds the lock on refs/heads/feature/x", "since":
  "2026-10-23T09:16:40Z"}}`. `at` is the revision that host's branch
  holds, `null` when it has none. Each host writes only its own entry,
  and takes it out on the round that sets its branch to `landed`.

All three are left out when there is nothing to say, so
`sluice/branch@1` is unchanged, and an older sluice reading them passes
over them.

`covered_by` names the result that covers the revision the combinations
are formed on, which each landing beside the queue names in its commit:
`{"at": "b7d2e94f031c5a8e6d2f40b9a7c3e1d58f6a2b47", "result": "passed",
"ran": 2, "replayed": 31}`. `at` is the revision of the branch that
result landed as, in full, and the rest are its own words. A landing
through the queue sets it to its own result, and a landing beside keeps
it, so landings beside in a row all name the same result. It is left
out when something other than sluice moved the branch, or a landing
changed the gate. A record an older sluice wrote has none;
the branch then lands nothing beside the queue until its next landing
through the queue names a result.

`refused_writes` holds the writes sluice's protection refused on the
branch, one for each host, ref and command, each host writing its own:
`[{"host": "build-1", "ref": "refs/heads/main", "command": "git -C
/home/me/app update-ref refs/heads/main <revision> <revision>",
"times": 2024, "first": "2026-10-30T05:50:23Z", "last":
"2026-11-02T16:01:20Z", "from": "10e4791a2b", "to": "8079db3c55"}]`.
`command` is the command as git ran it, each full revision in it
written as `<revision>`. `from` and `to` are the last refused write's,
`to` `null` for a deletion. A command leaves the list with its
`protected-ended` event. `sluice status <branch>` and `sluice overview`
show those refused in the last day; read them there, since this form
is sluice's own. The field is left out when it is empty.

`proof_missing` holds, on a branch whose state is on its remote, each
reason a host's runs kept no proof of a test that passed, while it
stands, one for each host and reason: `[{"host": "build-1", "why": "it
passed, but nothing the run kept proves it ran: moon's manifest of it
is not in /home/me/app/.moon/cache, where moon kept none of the run's
manifests", "since": "2026-11-09T09:16:40Z", "runs": 3}]`. `since` is when the first such run was read, and `runs`
counts that host's runs it touched. A reason leaves the list with its
`proof-kept` event. No round, gate or landing reads it. The field is
left out when it is empty, and an older sluice passes over it.

## A submission

```json
{
  "schema": "sluice/submission@1",
  "id": 13,
  "branch": "feature/concurrent-units",
  "head": "8d41b6c2e5",
  "from": { "agent": "agent-2", "worktree": "batch-review",
            "path": "/home/me/wt/unit-batch-review", "host": "build-1",
            "session": "herdr:w1:p2@/tmp/herdr/herdr.sock" },
  "submitted": "2026-10-11T09:16:40Z",
  "after": [],
  "priority": 0,
  "attempt": 1,
  "state": "failed",
  "tested": { "combination": "e20a6f9b48", "on": "c41e8f2d90", "submissions": [12, 13] },
  "result": "failed",
  "failed": [ { "task": "tests:t-batch-cards", "hash": "5c0e97aa21",
               "log": ".git/sluice/.logs/feature/concurrent-units/c7/tests:t-batch-cards.log" } ]
}
```

`state` is one of `queued`, `waiting` (behind the submissions in
`after`), `testing`, `passed`, `landing`, `landed`, `failed`,
`conflict` and `withdrawn`. These nine are stable: under
`sluice/submission@1` none of them is renamed or removed, and none is
added, so a script that reads `state`, such as an agent looping on
`sluice wait --json`, can match all of them.

`failed` lists each test that failed, with its `hash`, the key its
result is recorded under, and its `log`, where what it printed is kept.
A task a required test depends on is listed among them where it ran and
failed, and the task runner skipped the required test for it: that task
is the one to fix. The required test is listed too, with `failed_by`
naming the tasks it depends on that failed, and no `hash` or `log`,
since it never ran:

```json
"failed": [
  { "task": "api:integration", "hash": "8c41a0e7d2",
    "log": ".git/sluice/.logs/main/c4/api:integration.log" },
  { "task": "root:integration", "failed_by": ["api:integration"] }
]
```

A test with neither `hash` nor `failed_by` was left without a result:
skipped for another reason, never reached or cut off by the time limit.
Or it could not run, and then its `not_run` says why. A test read as
failed because its key both passed and failed has `flaky`, as
[`sluice overview --json`](/sluice/reference/commands/#sluice-overview)
gives it. A record written by an older sluice has no `failed_by`.

On a branch whose state is on its remote, a test is failed under a key
the combination's tree doesn't bear out. Its `unsigned` gives why, in
the words of the profile that read the task runner's record of it. A
record written by an older sluice has no `unsigned`:

```json
"failed": [
  { "task": "tests:t-batch-cards", "hash": "5c0e97aa21",
    "unsigned": "tests:t-batch-cards: tests/batch.txt is not in the tree" }
]
```

`after` lists submissions this one must land after. `attempt` counts how
many times this worktree has submitted to this branch. `from.worktree`
is the branch the work was submitted from; `from.path` is the worktree
itself, for a reader to match a submission to the session that made it.
A submission written by an older sluice has no `path`.

`from.host` is the host the submit ran on, by the name its peers know
it by, and `from.session` is the session it ran in. Under herdr that's
`herdr:<pane>@<socket>`: the pane and the herdr server it's on. A
submit run in no herdr pane has no `session`. The round that decides
the outcome types a notice into that pane, one line to the agent there,
when the round runs on the same host:

```text
[sluice] #13 failed on feature/concurrent-units: tests:t-batch-cards. sluice wait 13 shows why. sluice confirm 13:failed once you have read it.
```

The notice ends with the key the agent confirms the outcome by, once it
has acted on it. The outcome itself is kept in the branch's
[`deliveries.jsonl`](#outcomes-owed), owed to the work that submitted
it until that work confirms it, and a submission's record holds no mark
of it. `sluice status <id> --json`, a wait's `--json` and each element
of `sluice inbox --json` give a decided submission's `delivery`, where
it stands:

```json
"delivery": { "key": "13:failed", "reader": "batch-review", "state": "owed",
              "recorded_at": "2026-10-11T09:21:50Z", "expires_at": "2026-10-18T09:21:50Z",
              "handed": [ { "by": "notice", "at": "2026-10-11T09:21:55Z" } ],
              "confirmed": null, "known": true }
```

`state` is `owed`, `expired` once it has gone unconfirmed past its
expiry, or `done` once its reader has confirmed it, at `confirmed`.
`handed` lists each time it was handed over and how; `known` says
whether anything has been confirmed under the reader's name. One done
and let go of gives its `key` and `state` alone. A submission by name,
a withdrawn one and one not decided have none.

A submission written by an older sluice may hold `delivered` and
`delivered_to`, or `told` and `told_to`: sluice reads none of them, and
drops them the next time it writes a live submission. An outcome the
round has yet to push waits in `branch.json`'s `outbox`, each item
holding it under the key `told`, which an older sluice on another host
still reads.

A submission that landed beside the queue, because no required check
reads its changes, has `"beside": true`, and `covered_by`, the result
that covered the revision it landed on, in the form `branch.json` gives
it. Its notice reads `[sluice] #15
landed on feature/concurrent-units as 4e1a07c beside the queue: no
required check reads its changes.` A submission can wait all the same,
though no check reads its changes on the branch as it is. Then it has
`"beside": { "behind": 13, "why": "read" }`, naming the submission
whose combination's checks read it. With `"why": "conflict"` it names
the submission whose work it conflicts with. A submission that a check reads on the branch as it is
has no `beside`. A submission made with others in one act has `"act"`,
the id of the act's first.

A passed submission waiting under its branch's [`hold`](/sluice/reference/configuration/#hold)
has `"held": { "by": [12, 13, 14], "until": "2026-10-11T09:26:40Z" }`:
the longer combination it waits for, by its submissions, and when the
hold runs out. Nothing reads it to decide; the round decides from the
branch's record, where each combination that passed has `passed_at`.

A submission that has landed or been withdrawn moves, unchanged, from
`submissions/` to `.finished/` in the same write that records its outcome,
once no combination or landing still names it. A round, `sluice
overview`, `sluice` alone and `sluice status <branch>` read only the
branch's own record, `submissions/` and `finished.jsonl`, so they cost
the size of the queue, not of the branch's history. `sluice status <id>`
reads a finished submission's one file, and `sluice status <branch>
--all` reads them all. Records written by an older sluice move on the
branch's next write.

A pull request's submission comes from `pull/<n>`, its number on
GitHub, in place of a worktree: `"from": { "agent": "agent-2",
"worktree": "pull/41", "head": "cards" }`, with no `path`; `head`
is the branch the pull request was opened from. Its agent is the one the pull request's
description names, in the comment `sluice submit` writes there, or
else whoever opened it. Each new commit on the pull request is a new
submission from the same `pull/<n>`. A pull request still waiting for
review or checks has no submission at all: its state is read from GitHub
each round, and shown there as the `sluice` check. Submission ids
are numbered in order, and that order is the queue's order.

A [submission by
name](/sluice/reference/commands/#submitting-a-branch-by-name), `sluice
submit main --head feature/runner`, names the branch as its work and
its head: `"from": { "agent": "feature/runner", "worktree":
"feature/runner", "head": "feature/runner", "host": "build-1" }`. It
has no `path` and no `session`, wherever the submit ran. Its agent is
`SLUICE_AGENT` where the submit's session set it, and the branch
otherwise. `from.head` equal to `from.worktree` marks a submission by
name, and no other field does: a worktree's own submission has no
`head`, and a pull request's `worktree` is `pull/<n>`. Its outcome goes
to no session, so it is owed to no one: `deliveries.jsonl` holds
nothing for it, it gives no `delivery`, and the log holds no `pending`,
`handed` or `confirmed` event for it. A worktree that has
checked out the branch submits the same work, by `from.worktree`, so
each withdraws the other's earlier submission and `attempt` counts both.
To a branch with pull requests, the submission is the pull request's,
from `pull/<n>` with `head` the branch.

A withdrawn submission has `"withdrawn": { "by": "operator", "at":
"2026-10-11T09:30:02Z" }`: who withdrew it, and when. An older sluice
withdrew a host's own submissions of its branch's pull requests when it
found another host taking them in, a hand-over, and gave such a
submission's `withdrawn`, and its log line, `"handed_over_to": "build-2"`,
that host; one older still gave none, and its `by` reads `build-2 takes
main's pull requests in`. sluice reads `handed_over_to` and no longer
writes it. Such a submission holds no pull request: the host that keeps
the branch submits it again at its latest commit.

## The log

One event per line. A test is logged when it runs, a failure exactly
like a pass, and a replayed result is logged as a replay, never as a
run:

```json
{"schema":"sluice/log@1","event":"ran","combination":"9ab3d07c11","task":"tests:t-after","hash":"4d1e22b0c9","outcome":"pass","host":"build-1"}
{"schema":"sluice/log@1","event":"replayed","combination":"9ab3d07c11","task":"tests:t-amend","hash":"77aa01f3de","outcome":"pass"}
{"schema":"sluice/log@1","event":"ran","combination":"e20a6f9b48","task":"tests:t-batch-cards","hash":"5c0e97aa21","outcome":"fail","host":"build-1"}
{"schema":"sluice/log@1","event":"blamed","id":13,"combination":"e20a6f9b48"}
{"schema":"sluice/log@1","event":"landing","ids":[12],"from":"c41e8f2d90","to":"b7d2e94f03"}
{"schema":"sluice/log@1","event":"landed","ids":[12],"to":"b7d2e94f03"}
```

`landing` is written before the branch moves and `landed` after, so a
round that crashes between the two is finished correctly by the next
one.

`handed` is logged each time a submission's outcome is handed over,
with its key, `by` the way, `notice` (typed into the session's pane),
`pull-request` (posted on the request), `inbox` or `wait`, and `to`
where it reached: the pane, the request, the worktree's path or the
agent. `confirmed` is logged when its reader confirms it, with the
`reader` and `by` who: `confirm`, `operator`, `resubmission` (the
worktree submitted again), or the way that confirmed it on receipt.
`pending` is logged once when the round's push isn't taken, with `why`
in the words of whatever didn't take it, and no round tries again. A
log written by an older sluice may hold `delivered` lines; they read as
they were written:

```json
{"schema":"sluice/log@1","event":"handed","id":12,"key":"12:landed","by":"notice","to":"herdr:w1:p2@/tmp/herdr/herdr.sock"}
{"schema":"sluice/log@1","event":"confirmed","id":12,"key":"12:landed","reader":"batch-review","by":"confirm"}
{"schema":"sluice/log@1","event":"pending","id":13,"notification":"13:failed","why":"agent is blocked"}
```

## Outcomes owed

`deliveries.jsonl` holds each decided outcome a worktree or a pull
request is owed, one JSON record per row, in the order added. The round
that decides a submission landed, blamed or in conflict records it
once, under its key, `<id>:landed`, `<id>:failed` or `<id>:conflict`.
It is owed to the submission's work, the worktree's branch or `pull/<n>`,
with the notice as its message, and expires after the host's
[`[deliveries] expire_after`](/sluice/reference/configuration/#deliveries),
a week where unset. Each hand-over and each confirmation is a record of
its own beside it, and so is a name the first time anything is confirmed
under it:

```json
{"format": "weir-delivery/1", "key": "12:landed", "readers": ["batch-review"], "message": "[sluice] #12 landed on feature/concurrent-units as b7d2e94.", "recorded_at": "2026-10-11T09:21:50Z", "expires_at": "2026-10-18T09:21:50Z"}
{"format": "weir-handover/1", "key": "12:landed", "reader": "batch-review", "by": "notice", "at": "2026-10-11T09:21:55Z"}
{"format": "weir-confirmation/1", "key": "12:landed", "reader": "batch-review", "at": "2026-10-11T09:40:12Z"}
{"format": "weir-known/1", "reader": "batch-review", "known_at": "2026-10-11T09:40:12Z"}
```

An outcome nobody confirms stays, expired, and `sluice status <branch>`
shows it under its reader until someone confirms it, the operator with
`sluice confirm <key> --as operator` for a worktree that is gone. Once
every reader has confirmed it, it is let go of whole, its hand-overs and
confirmations with it, by the write that moves its submission out of
the queue or the first write after; a name stays known. A line sluice
cannot read stops the branch's reads, naming `deliveries.jsonl` and
what was wrong with it.

`held` is logged once for each revision a held branch is read at, with
the landing it lacks, and `accepted` when the operator accepts that
revision with `sluice branch accept`:

```json
{"schema":"sluice/log@1","event":"held","read":"10e4791a2b","lacks":"c43609b8e1"}
{"schema":"sluice/log@1","event":"accepted","revision":"10e4791a2b","lacked":"c43609b8e1"}
```

`copy-behind` is logged when a round can't set this host's own branch
of a branch kept on a remote to its landing. It's logged once for each
revision that branch holds, landing and reason. `copy-levelled` is
logged when a round sets one that was behind. `copy` is the revision
this host's branch holds:

```json
{"schema":"sluice/log@1","event":"copy-behind","host":"build-2","copy":"10e4791a2b","lacks":"c43609b8e1","why":"git holds the lock on refs/heads/feature/x"}
{"schema":"sluice/log@1","event":"copy-levelled","host":"build-2","copy":"c43609b8e1"}
```

`read-refused` is logged when a round can't read the branch, or its
state, from the branch's remote. It names the host, the remote, the ref
the round asked for and what git said, whole. It's logged once for each
ref and words while that host's rounds are refused, never once a round,
and each host logs its own. The first round on that host that reads the
remote again logs `read-again`. `since` is when the first round was
refused, `stood` how many seconds since then, and `rounds` how many of
that host's rounds it refused:

```json
{"schema":"sluice/log@1","event":"read-refused","host":"build-1","remote":"origin","asked":"refs/heads/main","said":"remote: Repository not found.\nfatal: repository 'https://github.com/acme/app.git/' not found"}
{"schema":"sluice/log@1","event":"read-again","host":"build-1","remote":"origin","since":"2026-10-26T09:16:40Z","stood":1800,"rounds":12}
```

`lost` is logged when a run has no outcome: its executor stopped before
it reported, or it left tests not run and failed none on its own
result. Its combination is tested again. `not_run` lists each test the
run left not run, with its `reason`. A test that passed in a run that
kept no proof of it also has `key`, the key of the manifest that's
missing. That's the test's own key, or, as its reason names, the key of
a task it depends on. A line written by an older sluice has no `key`. A
test that did not pass in a run the sandbox refused something names what
was refused:

```json
{"schema":"sluice/log@1","event":"lost","combination":"e20a6f9b48","not_run":[{"task":"tests:t-after","reason":"it passed, but nothing the run kept proves it ran: moon's manifest of it is not in /home/me/app/.moon/cache, where moon kept none of the run's manifests","key":"4d1e22b0c9"}]}
{"schema":"sluice/log@1","event":"lost","combination":"e20a6f9b48","not_run":[{"task":"site:install","reason":"sluice's sandbox refused the run network-outbound /private/var/run/mDNSResponder (a name lookup, by bun)"}]}
```

`detail` is logged after a run with what its runner said of it beyond
its results. `detail` holds how long the run took, its estimate and the
files it read as changed; `kept_out`, the required tests it keeps out of
its CI mode. Where sluice couldn't read what the sandbox refused the
run, so read it as refused nothing, `refusals_unread` says why. No
verdict reads this event:

```json
{"schema":"sluice/log@1","event":"detail","combination":"e20a6f9b48","refusals_unread":"log: exited 1 before it was ready"}
```

`unproven` is logged when a run replayed tests whose keys no result the
branch counts proves, so they run again with no replay. `tasks` names
them. `keys` names each with its `task`, its `key` and `why`, the first
of these that holds:

- `no run of its key is recorded`;
- `its key ran only on another system`;
- `no result of its key is signed by a signer the branch counts`, on a
  branch whose state is on its remote, or `no result of its key is by a
  run the branch trusts`, on a branch in a host's own files;
- `no result of its key was held to this run's limits`;
- `no result of its key was run under this run's grant`;
- `no result the branch counts for its key is a pass`.

A line written by an older sluice has no `keys`:

```json
{"schema":"sluice/log@1","event":"unproven","combination":"9ab3d07c11","tasks":["tests:t-amend"],"keys":[{"task":"tests:t-amend","key":"77aa01f3de","why":"no result of its key is signed by a signer the branch counts"}]}
```

`proof-missing` is logged when a run on a branch whose state is on its
remote passed a test and kept no proof of it, so the pass has no result.
It names the host that read the run and why. The reason names whose
manifest is missing, the test's own or one of a task it depends on, and
the store it was looked for in. It also says what that store shows of
itself. Either the store doesn't exist, or the user sluice runs as can't
write it or a directory of it. Otherwise it says whether moon kept
others of the run's manifests there. It's logged once for each reason while that
host's runs keep no proof, never once a run, and each host logs its
own. The first run on that host that proves a test that passed, and
leaves none without proof, logs `proof-kept`. `since` is when the first
such run was read, `stood` how many seconds since then, and `runs` how
many of that host's runs it touched. A run that passed nothing ends
nothing:

```json
{"schema":"sluice/log@1","event":"proof-missing","host":"build-1","why":"it passed, but nothing the run kept proves it ran: moon's manifest of it is not in /home/me/app/.moon/cache, where moon kept none of the run's manifests"}
{"schema":"sluice/log@1","event":"proof-kept","host":"build-1","why":"it passed, but nothing the run kept proves it ran: moon's manifest of it is not in /home/me/app/.moon/cache, where moon kept none of the run's manifests","since":"2026-11-09T09:16:40Z","stood":1800,"runs":3}
```

`protected` is logged once for each command a host's protection
refused, with the `host`, the `ref`, its `from` and `to`, and `by`, the
command as git ran it. `to` is `null` for a deletion. Each later write
of that command refused on that host is counted in `branch.json`'s
`refused_writes`, with no event. A day after the last of them,
`protected-ended` is logged with `times`, how many were refused, and
the `first` and the `last`. The next write to the state logs it, at
the time the day ended, and the command's next refused write is logged
as `protected` again. When one host has more than twenty commands
counted on the branch, the one refused longest ago ends at once:

```json
{"schema":"sluice/log@1","event":"protected","ref":"refs/heads/main","from":"10e4791a2b","to":"8079db3c55","by":"git -C /home/me/app update-ref refs/heads/main 8079db3c55e9d71f5ee7c92d6dc9e92ffdad17b8 10e4791a2b86f7e437faa5a7fce15d1ddcb9eaea","host":"build-1"}
{"schema":"sluice/log@1","event":"protected-ended","host":"build-1","ref":"refs/heads/main","command":"git -C /home/me/app update-ref refs/heads/main <revision> <revision>","times":2024,"first":"2026-10-30T05:50:23Z","last":"2026-11-02T16:01:20Z"}
```

A round that can't read the branch or its state forms, starts and lands
nothing, and says why to whoever ran it, as before. When the state is on
the remote that refused it, the round can't write these events either.
The host keeps them, and its first write to the state after writes them,
each with the time it happened. So an event written late can read
earlier than the one before it.

`released` is logged when a combination's checkout is released. One
freed at the operator's `--accept-failure` carries `accepted`, what had
failed of its release:

```json
{"schema":"sluice/log@1","event":"released","place":"sluice/try/main/48","accepted":"the release command of /home/me/repo/.git/sluice/.places/sluice+2Ftry+2Fmain+2F48 ended exit status: 2: usage: scripts/preview up"}
```

`release_failed` is logged for each try of a release that failed, with
what failed and how many tries so far; `held` is true on the try after
which it is tried no more. `gone` is logged when a run or round frees a
checkout whose directory no longer exists, or that git no longer lists
as a worktree. Before it frees one, it runs the release command the
checkout's last run stored, once, from its scratch directory. `ran`
says whether its runs stored one to run, and `failed` what it printed
when it failed; the checkout is freed either way and never tried again.
For a worktree it also names the worktree:

```json
{"schema":"sluice/log@1","event":"release_failed","place":"sluice/try/main/48","failed":"…: usage: scripts/preview up","tries":4,"held":true}
{"schema":"sluice/log@1","event":"gone","place":"session//home/me/wt/unit-c","worktree":"/home/me/wt/unit-c","ran":true}
```

While a checkout's release is failing, `branch.json` keeps it in
`failed_releases`: its `place`, what `failed`, its `tries`, when it was
last tried (`at`), `stopped` once it is held, and on a branch whose
state is on its remote the `host` that made it. It leaves the list once
it is released, freed or found gone. A worktree's or a preview's own
release that failed is kept on its record in `.places/` or `.previews/`
as `release_failed`, since those are the host's alone.

Full example files are in the repository under
[docs/examples/state](https://github.com/agentplot/sluice/tree/main/docs/examples/state).

## The version

`version` is bumped on every write, and a round writes only if the
version is still the one it read. Two rounds never both write; the one
that loses reads again. On a branch whose state is on its remote, the
same rule is a push with an expected old value, and it holds between
hosts as it does between rounds on one
([above](#a-branch-whose-state-is-on-its-remote)).

## What a landing looks like in history

One merge commit per submission: first parent the branch, second parent
the agent's head, so the agent's commits stay in history and `wt list`
shows its worktree as merged. What was tested rides along as trailers,
which outlive the state files:

```console
$ git log -1 feature/concurrent-units
commit b7d2e94f03
Merge: c41e8f2 3f9c2e1
Author: sluice <sluice@localhost>

    chore(land): #12 touches

    Sluice-Submission: 12
    Sluice-From: agent-1 (touches)
    Sluice-Tested: 9ab3d07c11
    Sluice-Result: passed (2 run, 31 replayed)
```

A landing beside the queue names no combination, since none was run.
Its `Sluice-Result` says so, then names the result it stands on:

```console
$ git log -1 --format=%B feature/concurrent-units
chore(land): #15 guide

Sluice-Submission: 15
Sluice-From: agent-4 (guide)
Sluice-Result: passed (0 run, 0 replayed) beside the queue: no required check reads its changes; covered by the result at b7d2e94f031c5a8e6d2f40b9a7c3e1d58f6a2b47, passed (2 run, 31 replayed)
```

The first word of `Sluice-Result` is the landing's own result. It is
`passed` on every landing, since a branch moves only on a result that
passed, and the counts after it are what that landing ran and replayed.
A landing beside ran nothing, because no required check reads its
changes. The rest of the trailer names the result that covers the
revision it landed on: the revision of the branch that result landed
as, in full, and that landing's own words. A tool that reads a
branch's head as passed reads the first word.

The landing commit carries exactly the tree that was tested
([ADR 0006](/sluice/decisions/0006/)), so when the submissions ahead of a
combination land as they were tested, the combination is the same one on
the new revision and is not tested again.
