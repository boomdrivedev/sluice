# Design

## Context

See proposal.md, Why. The code as it stands:

- `Sluice::session_request` (`crates/sluice-core/src/session.rs`) asks
  for `since: Some(plan.base)`, the branch's revision, and
  `replay: false`. Its comment says a session run is "never answered
  from the cache, since what a session run records must be observed
  (16)". Clause 16 says only that a replay is never recorded as a run,
  and `record_session` already records `Source::Ran` results alone. No
  record decided the `--force`.
- `Moon::plan` (`crates/sluice/src/profiles/moon.rs`) narrows the
  required checks when `since` is given. First it asks whether moon's
  own configuration changed, by `changed_files(place, since, revision)`,
  git's diff of the two revisions. Then it asks moon for the affected
  tasks with `MOON_BASE` and `MOON_HEAD` set to them. With
  `replay: false` it adds `--force`.
- moon 2.6.1, the release `.prototools` names, was probed on
  2026-11-19 in a scratch workspace with projects `a` and `b`:
  - Given `MOON_BASE=main` and `MOON_HEAD=unit`, where `unit` changed
    `b/lib` and `main` later changed `a/lib`, `moon query tasks
    --affected` reads from the merge base and gives `b` alone. git's
    diff of the two revisions gives both files.
  - With a revision range, moon reads nothing a worktree holds beyond
    its head. An unstaged change to `b/lib` gave no task.
  - `moon query tasks --affected` reads changed files from its standard
    input, as `{"files": ["a/lib"]}` or as one path a line, and gives
    `a` alone.
- A combination's run asks for `replay: true` (`step.rs`,
  `start_run`). Its report goes through `verdict::read`, which lists in
  `unproven` each replay whose key holds no pass the branch counts,
  under the run's limits, grant and system, with why (108, 138). The
  round runs those checks again with no replay.
- `Session.dirty` holds what a worktree has beyond its head, as
  `git status --porcelain=v1 -z --untracked-files=all` gives it, read
  before the run. `exec::path_of` reads a line's path.
- On this repository's branches, the signer `sessions` is the host's
  own key, so a clean worktree's recorded results are signed and counted
  (`.config/sluice.toml`, `trust = ["sessions"]`).

## Goals / Non-Goals

**Goals:**

- A worktree's run reads the same checks the queue's combination of its
  head would read, from the same proof of a replay.
- One affected reading in the moon profile for a combination and a
  worktree, and one replay proof in the core for both.

**Non-Goals:**

- Changing which checks a combination's run reads, or how a
  combination's replays are proven.
- A bar for a worktree's replay of its own: a worktree's run counts
  nothing the gate would not.
- Anything of `sluice exec <branch> -- <command>`.

## Decisions

### D1. Its own changes start at the merge base

The branch seam gains `merge_base(a, b) -> Result<Option<Rev>>` beside
`contains`. `local_refs` answers by `git merge-base`, `None` where the
two share no commit. The routing and forge profiles hand it on as they
hand on `contains`. The fakes and the conformance profile answer it too.
`Sluice::session_since(plan, session)` gives the merge base of the
session's head and `plan.base`. `session_request` takes it as `since`.
With `None`, every required check runs (70), and exec says so (D6).

Alternatives:

- **Keep `since` at the branch's revision and rely on moon's own merge
  base.** Rejected: sluice's own check for moon's configuration still
  diffs the two revisions and finds the branch's `moon.yml` changes. A
  revision range also holds none of the worktree's uncommitted paths.
- **Have the moon profile find the merge base in the place.** Rejected:
  the opening line needs it before the run, and the core and the profile
  would each hold a reading of one fact.

### D2. What the worktree holds beyond its head travels with the request

`RunRequest` gains `beyond: Vec<String>`: the paths the worktree holds
beyond its head, read from `Session.dirty` as `exec::path_of` reads
them. A rename gives both its paths where `dirty` keeps both.
`session_request` fills it for a run of the checks. Everywhere else it
is empty: a combination's run, a preview, a command, and a run again
(D5). A checkout sluice made holds nothing beyond its revision.

Alternative: **run `git status` in the place when the run is planned.**
Rejected: the checkout's equipping (rule core.46) has run by then and may
have written untracked files. The list read before the run is also the
one clause 92 decides recording by.

### D3. moon is handed the files, not a range

`Moon::plan`, given `since`, reads the changed files once. They are
`changed_files(place, since, revision)` with `beyond` added. `since` is
an ancestor of `revision` for a combination (its `on`) and for a
worktree (D1), so git's diff of the two revisions is the merge base's.
The check for moon's own configuration reads that list. Unless it finds
a change there, `Moon::tasks` asks `moon query tasks --affected
--downstream deep` with the list on moon's standard input, as
`{"files": [...]}`, and sets neither `MOON_BASE` nor `MOON_HEAD`. A
combination's targets stay what they were, since its list is what moon's
range gave. `Moon::reads` (140) keeps its revision pairs: it reads no
worktree, and its pairs are ancestors as well.

Alternative: **stdin for a worktree, the range for a combination.**
Rejected: that is two affected readings of one profile.

### D4. A worktree's run replays, and its replays are read as a combination's

`session_request` asks for `replay: true` for a run of the checks, so
moon runs without `--force`, as for a combination. `finish_session`
reads the run's report through `verdict::read`. It reads it against
`results_for(plan.branch, …)` for the keys the run replayed, under
`plan.rules`, with the host's own system and nothing given to prove.
From that reading it takes the replays left in `unproven`, and for each
proven replay how its key reads: passed, or failed as `flaky` reads a
pass beside a fail. `Finished` carries the unproven replays for exec.
Nothing of the reading is recorded or decides anything (rule core.18;
exec-checks, "Checks decide nothing"). `record_session` records the
`Source::Ran` results alone, as it does today (16; rule core.9).
`last_result.reused` keeps its meaning: once D5 has run the unproven
replays again, every replay left is proven by a pass the gate counts.

Alternatives:

- **A bar of the worktree's own: any pass sluice recorded, counted or
  not.** Rejected: the passes the gate counts are what the combination
  of the head will replay. On this repository's branches the `sessions`
  signer signs a clean worktree's results, so a worktree's own recorded
  pass already proves its replay.
- **Narrow the affected reading and keep `--force`.** Rejected: a check
  sluice already passed on the same files would still run.

### D5. An unproven replay runs again in the same `sluice exec`

Where `Finished` names unproven replays, exec says `replayed with no
result sluice recorded: <task>, …; running them again`. It then starts a
second session run in the same worktree:

- **The request.** `required` is those tasks, `since` is `None`,
  `replay` is false and `beyond` is empty.
- **Room.** The run asks the host process for room as the first run did,
  under `RunKind::Session` (rule core.49). The start loop in
  `Exec::exec` becomes a function both runs call. Each run keeps its own
  `begin_session` record and its own Ctrl-C handling.
- **Reading and recording.** Each run is read by `after_run` and
  recorded by `record_finished` on its own, under clause 92's
  conditions.
- **What the session sees.** The summary, `--json`, the exit and the
  worktree's `last_result` are of the two runs joined, the second run's
  tasks in place of the replays they ran.

A run again replays nothing, so it never runs again itself.

Alternatives:

- **Hold the first run's room for the second.** Rejected: a run counts
  toward the bound for as long as it runs (89), and rule core.49 hands
  out room in turn.
- **Leave the unproven replays not run and exit 1.** Rejected: the
  agent's next `sluice exec` would meet the same replay.

### D6. The words

- **The opening line.** It becomes `running the checks the worktree's
  own changes affect since <merge base> on <branch>`. Where the merge
  base is not the branch's revision it adds `, now at <branch's
  revision>`. With no merge base it reads `running every required check:
  the worktree's head shares no history with <branch>`.
- **The pass line.** It reads `passed: N tasks (K replayed)`, and stays
  `passed: N tasks` where K is 0.
- **`--json`.** The document gains `since`, the merge base's full
  revision or `null`. Each entry of `tasks` and `dependencies` gains
  `replayed`.

The words are exec-checks' requirements.

### D7. The record, the rule and the pages

The record is new, under the next number no branch of the repository
holds, named `a-worktrees-checks-read-its-own-changes-and-replay-what-sluice-passed`.
It holds D1, D4 and D5 with their alternatives. Its deciders are the
brief of this change and its construct stage, and its status is
accepted, as record 0126's is. Rule core.9's details gain two clauses,
citing the record:

- **Not ruled out.** A worktree's run answered from the runner's cache
  where a pass the gate counts is recorded for the key.
- **Rules out.** A worktree's replay told as passed with no such pass.

Four pages say the run is forced past moon's cache, reads the branch's
tip, or open with the old line. They say what this change makes true
instead:

- `site/src/content/docs/reference/commands.md`
- `README.md`
- `plugins/sluice/skills/using-sluice/references/exec.md`
- `plugins/sluice/skills/using-sluice/SKILL.md`

## Risks / Trade-offs

- **A later moon reads its standard input otherwise.** → The moon
  profile's test that hands moon a file list fails on the version bump
  that changes it, before any branch takes the bump.
- **Without `--force`, moon's CI mode narrows the targets itself.** → A
  combination's run already runs these targets without `--force`. The
  exec tests "The same files again" and "After a fix" run the
  worktree's form and name the checks it ran.
- **A host whose worktrees do not share moon's cache.** → A worktree
  replays only what its own earlier runs left. That is still never more
  than today's runs, and the host's `[[caches]]` decide the sharing as
  they do for combinations.
- **A worktree that never commits.** → Its passes are never recorded
  (92), so each of its replays runs again (D5). It takes no longer than
  today's forced run. The words tell it why.
- **The run again waits for room a second time.** → It happens only
  where moon's cache holds a pass sluice never recorded. exec says so
  before it waits.

## Migration Plan

Nothing stored changes form. `RunRequest.beyond` is internal to the
binary. The `--json` fields are added, none removed. A host takes the
change with the release that carries it.
