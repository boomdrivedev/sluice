# Verdicts, culprits and conflicts

## Submission states

| state | meaning |
|---|---|
| `queued` | in the queue, not under test |
| `waiting` | an `--after` submission is neither landed nor ahead of it |
| `testing` | in a combination under test |
| `passed` | its combination passed; it lands next |
| `landing` | its landing is written and the branch is being moved |
| `landed` | on the branch; `landed_as` is the landing commit |
| `failed` | blamed for a failure |
| `conflict` | does not merge with what is ahead of it |
| `withdrawn` | taken out, by its agent, a planner, the operator or a resubmission; an older sluice's hand-over to the host that took its pull requests in names that host in `withdrawn.handed_over_to` |

`failed` and `conflict` are out of the queue until you submit again.
These nine are all the states there are: `sluice wait --json` and
`sluice status <id> --json` give one of them as `state`, and none is
renamed, removed or added under `sluice/submission@1`. Decided means
`landed`, `failed`, `conflict` or `withdrawn`.

## Reading a failure

```
failed: tests:t-batch-cards
  on feature/x + #12 + #13 (e20a6f9)
  #12 passed on its own, so the failure is from this submission
  log of tests:t-batch-cards: .git/sluice/.logs/feature/x/c7/tests:t-batch-cards.log
the agent working in batch-review fixes it there and submits again
```

- sluice blames one submission, and only after every combination ahead of it
  has a result. If the combination ahead passed, the failure is yours.
- `on <combination>`: the exact revision tested, the branch plus every
  submission ahead plus yours. A failure may come from an interaction with
  work ahead of you that passes alone.
- The log path is under `$(git rev-parse --git-common-dir)/sluice/.logs/`
  (`.git/sluice/` in a normal clone, `project.git/sluice/` in a bare one). The last 20 failed combinations keep
  their logs.
- `timed out`: the run hit the branch's `timeout`. Tests that ended before
  it keep their results. A test still running then is named, with its log,
  and nothing is recorded under its key:

  ```
  timed out: scripts/ci:test was running when the time limit ended the run
    on feature/x + #33 (990de53)
    log of scripts/ci:test: .git/sluice/.logs/feature/x/c7/scripts%2Fci:test.log
  nothing was recorded for scripts/ci:test: if the host was slow, the agent working in ci submits the same head again; if scripts/ci:test never ends, it fixes it there and submits again
  ```

  Read the log. A test that stopped printing is waiting on something that
  never exits; a test still making progress on a busy host only needs the
  same head submitted again.
- `failed by <task>, a task it depends on`: the task runner skipped a
  required test because a task it depends on ran and failed. That task is
  listed under `failed:` with its log, and the test itself never ran, so it
  has no log and is never `not run`. Read the task's log and fix the task:

  ```
  failed: api:integration
    root:integration failed by api:integration, a task it depends on
    on feature/x + #14 (e20a6f9)
    log of api:integration: .git/sluice/.logs/feature/x/c7/api:integration.log
  the agent working in cards fixes it there and submits again
  ```

  The task's fail is recorded under its own key, with its `hash`, and a
  flake of it is weighed there; nothing is recorded under the test's.
- A test with no result (skipped for another reason, never reached, or
  ended by the time limit) counts as failed and has no `hash` and no
  `failed_by`. One that never started has no log either.

From JSON:

```sh
sluice status 13 --json | jq '{state, result, tested, failed}'
```

`tested.combination` is the revision, `tested.submissions` who was in it,
`failed[].task` and `failed[].log` the tests and their logs. A task a
required test depends on that failed it is in `failed[]` with its `hash`
and `log`; the test it failed has `failed_by`, naming the task, and no
`hash`:

```json
[ { "task": "api:integration", "hash": "8c41a0e7d2",
    "log": ".git/sluice/.logs/feature/x/c7/api:integration.log" },
  { "task": "root:integration", "failed_by": ["api:integration"] } ]
```

## Tests a host cannot run as the policy asks

A host takes none of a branch's combinations when its executor cannot
hold the limits the branch's policy states (its `[executor] kind`,
`memory` or `network`), or, on a branch whose policy says `[executor]
docker = true`, when no container engine answers there. It leaves them
for a host that can, and nothing of them runs or is recorded there.
`sluice status` and `sluice wait` say what a combination waits for, and
`--json` carries it as `waiting`:

```
testing feature/x + #12 · 3m10s of 30m · waiting for a host that can run its tests as its policy asks: build-box: no container engine answers: no socket at /var/run/docker.sock
```

A limit reads the same way, in the executor's words:

```
testing feature/x + #12 · 3m10s of 30m · waiting for a host that can run its tests as its policy asks: mac-box: [executor] memory = 8589934592 bytes: the sandbox executor on macOS cannot hold a memory limit
```

When no host takes it before the branch's `timeout`, the combination is not
passed, and each test says why:

```
timed out
not run: cli:tools
  cli:tools could not run: no host that could run its tests as its policy asks took them: build-box: no container engine answers: no socket at /var/run/docker.sock
```

Start the engine where sluice runs, as the user it runs as, or serve the
branch from a host whose executor holds its limits, then submit again. A
test that did not pass, after which the engine no longer answers, has no
result: nothing is recorded under its key, nobody is blamed, and the
combination is tested again. A test that passed keeps its result.

```
could not run: cli:tools: after the run, no container engine answers: /var/run/docker.sock refused the connection
```

What a test prints is never read as the engine gone: a test that says it
cannot reach the daemon while the engine still answers is recorded as
failed.

## A test the sandbox refused something

On a branch whose `[executor] kind = "sandbox"`, a test that did not pass
in a run the sandbox refused something has no result. sluice reads what
the sandbox refused from its own report of the run, never from what the
test printed. Nothing is recorded under the test's key, nobody is blamed,
the combination is tested again, and a test that passed keeps its
result:

```
could not run: site:install: sluice's sandbox refused the run network-outbound /private/var/run/mDNSResponder (a name lookup, by bun)
nothing was recorded for a test that could not run: once the host can run it, run again
```

Here the test looked up a name under `network = "localhost"`, which
refuses every lookup. sluice's sandbox stopped it, not your code. The
cure is the branch's policy (a wider `network`), the host's `[[caches]]`
(a folder the test must write, declared for every run), or the test (one
that needs nothing the sandbox refuses). Never a set-aside: nothing was
recorded, so there is no result to set aside. After three such runs in a
row the newest submission is blamed, each test named with what was
refused.

A test that fails in a run the sandbox refused nothing is recorded and
blamed as ever. When sluice can't read what the sandbox refused a run, it
reads the run as refused nothing, records the fail, and says so beside
it:

```
sluice could not read what its sandbox refused the run: log: exited 1 before it was ready
```

## A failure that is not your code

A test can fail because of the host: docker down, a full disk, a slow
program start, a port held by a stray process. Check the host before you
touch code or resubmit.

A test that needs something on the host, a container engine, a database
or a stack, starts it and waits for it within the branch's timeout. A
repository tells sluice, in `[tests]`, how such a test says it could not:

- `not_run`: exits by which a test says it could not run here, such as
  `[77]`.

A test that could not run has no result. Nothing is recorded under its key,
the combination is tested again, and nobody is blamed:

```
could not run: cli:tools: exited 77, which [tests] not_run names: it could not run here
nothing was recorded for a test that could not run: once the host can run it, run again
```

When a combination's tests could not run three runs in a row, its newest
submission is blamed, and each test is named with why:

```
not run: cli:tools
  cli:tools could not run: exited 77, which [tests] not_run names: it could not run here
the host could not run cli:tools: once it can, the agent working in tools submits again
```

Tests that could not be started count the same way: the checkout could
not be made, `[tests] toolchain` or the task runner's setup failed, or
the executor turned down a limit the branch's policy states. Nothing runs
and nothing is recorded: the checkout is released, and any host, this one
included, may take the combination again. Since the tests never started,
each is named as the branch's `required` names it, and the run's words
say what failed:

```
not run: #gate
  #gate could not run: could not start: [tests] toolchain devenv print-dev-env exited exit status: 1: error: flake.lock is not up to date
the host could not run #gate: once it can, the agent working in tools submits again
```

The cure is the toolchain or the host. Fix what the words name in your
worktree when the repository holds it, a stale lock file or a toolchain
that no longer builds, and submit again; when it is the host's (a full
disk, a tool missing), tell the user what the host lacks. Nothing was
recorded, so there is no result to set aside.

sluice tells these apart on its own and records nothing for them: a run
that never started, a `not_run` exit, a test the time limit ended, a test
that did not pass in a run the sandbox refused something, and, on a
branch with `docker = true`, a test that did not pass after which the
container engine no longer answers. A host failure none of them catches is recorded like any fail, under the
test's key. A later pass on the same key then makes the
test read as flaky, and on a branch with `flaky = "fail"` the same head
fails each time it is submitted:

```
failed: checks:macbook-pro (flaky)
  on main + #63 (a557c99)
  checks:macbook-pro passed in this run and failed on the same inputs at 2026-10-17T15:28:04Z on 88de9be
  log of this run: .git/sluice/.logs/main/c64/checks:macbook-pro.log
  log of the run that failed: .git/sluice/.logs/main/c51/checks:macbook-pro.log
  if the host or a time limit caused that fail, the operator can set it aside: sluice set-aside …
  main reads a test that passed and failed on the same inputs as failed (flaky = "fail")
the same head lands once the operator sets the other result aside, or once a real fix changes what checks:macbook-pro reads
```

Read the log of the run that failed. When that fail looks like the host's
doing (the run timed out, or the host was overloaded or broken then), you
have one move: stop, and tell the user what you found. Give the test, the
time of each run, what each log shows and what the host was doing. Setting
the fail aside is the operator's ruling; once they have, submit the same
head again.

Never change what the test reads only to get it tested again. The fail
stays under its key for any tree that returns to those inputs, and the
change hides a failure sluice should have told apart. When the fail is the
test's own, the test is flaky: fix it in your worktree.

In JSON, such a test's `failed[].flaky` holds `here`, this run's outcome,
and `other`, the result that contradicts it: its `outcome`, `at`,
`branch`, `combination` and, when kept, `log`.

A pull request your submit opened, sent back (blamed, timed out, in
conflict or withdrawn), is marked a draft and waits for your next
submit: nothing else takes it again in your name. A hand-over, which
only an older sluice made, is none of these: the host that keeps the
branch takes it again, and a wait on it follows it there. Submit again from its
worktree, with a fix or with the same commits after a time-out, and
sluice marks it ready for review and tests it again like any other
submission. A pull request a person opened is tested again only on new
commits, or once it is marked a draft and then ready for review.

On GitHub, the pull request carries the same verdict as a report: one
comment in its conversation, the `sluice` check's details, and a check per
required test on each system (`sluice / app:test (linux-x64)`). Each test's
row says whether it ran or was replayed, where, how long it took and how
many tries; a failed test's output is posted only when the branch's policy
sets `log_lines`. A pull request tested together with a blamed one shows
none of that failure: its report says its combination is formed again.
The report decides nothing; read the failure from `sluice submit` or
`sluice status --json` as above.

## Reading a conflict

```
conflict: plugin/lib/plan.py
  conflicts with #12 (agent-1, touches)
the agent working in forecast merges or rebases onto feature/x there and submits again
```

Bring the branch (and, if it has not landed yet, wait for #12, or rebase on
it once it lands) into your worktree, resolve, commit, submit again. Run
`sluice submit --check <branch>` first to see whether it now merges.

## Recent landings and blames for the whole branch

```sh
sluice overview --json | jq '.branches[] | {branch, recent}'
```

`recent` holds the branch's last ten `landed`, `blamed` and `conflict` log
lines exactly as written.
