## Why

An agent's `sluice exec <base>` takes 6 to 35 minutes where the merge
queue's run of the same change takes 3 to 4. On 2026-11-18 one fix's
agent waited about 123 minutes on such runs, a full `sluice exec main`
among them at 21 minutes, while the queue ran the change's 13 tasks in
3.7 minutes (#283). That is ops' root-cause analysis of 2026-11-18,
cause 5, behind the user's constraint that a flake change reaches a
host in minutes (signal 2026-11-19-boomdrive-ops-b31633c8/01).

Two causes are sluice's:

- **A worktree's run never replays.** It runs every affected check
  with moon's `--force`, so a check sluice passed on the same files,
  and every task it depends on, runs again. A combination's run lets
  moon replay and reads each replay against the passes the branch's
  gate counts (108).
- **A worktree's affected reading takes the branch's work for its own.**
  sluice asks whether moon's configuration changed between the branch's
  revision and the worktree's head. That is a diff, so in a worktree cut
  before the branch last changed a `moon.yml` it finds a change and runs
  every required check. Edits the worktree has not committed are never
  read: moon is given a revision range, which holds none of them.

## What Changes

- **A worktree's checks are those its own changes affect.** Its own
  changes are the files that differ between where its head left the
  branch, the merge base of its head and the branch's revision, and the
  worktree as it stands: its commits since then, and every path it holds
  beyond its head, untracked files git does not ignore among them. moon
  reads the affected checks from those files, as it reads a
  combination's. A change to moon's own configuration among them still
  runs every required check.
- **A check sluice already passed on the same files is replayed.** A
  worktree's run lets the runner answer from its cache. Each replay
  stands only where the branch's records hold a pass the gate counts for
  its key, under the run's limits, grant and system. That is the proof a
  combination's replay is held to. A replay sluice holds no such pass
  for runs again within the same `sluice exec`, with no replay. A replay
  is never recorded as a run (16; rule core.9).
- **The output says it.** The opening line names the revision the
  worktree's changes are read from. The summary counts the replays.
  `--json` gives `since` and each task's `replayed`.
- **Unchanged.** A command after `--` runs as before. The script runner
  reads every check as affected and never replays (C.2). What is
  recorded, and when, stays as clause 92 has it. A combination's checks
  are the same checks as before.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `exec-checks`: a worktree's run reads the checks its own changes
  affect, replays what a counted pass proves and runs the rest again,
  and says so in its output.

## Out of scope

- **A branch revision no verdict covers (70).** A worktree's checks read
  its own changes there too, while the queue's next combination runs
  every check. Whether a worktree's run should then run every check is a
  change of its own.
- **The toolchain's files in a combination's run.** 140's reading
  (`own_setting`) counts a change to `devenv.nix`, `flake.lock` or
  `.prototools` as read by every check. A combination's run only counts
  moon's configuration. Bringing the two together is a change of its
  own. This change keeps the combination's rule for a worktree.
- **A code stage running the gate once before delivering.** That is
  Flume's half of ops' cause 5, owned by the fix kind's instructions.

## Impact

- `crates/sluice-seams/src/lib.rs`: `RunRequest.beyond`, the paths a
  worktree holds beyond its head; the branch seam's `merge_base`.
- `crates/sluice/src/profiles/local_refs.rs`, `pushed_refs.rs`,
  `branches.rs`, `github/pulls.rs`, `crates/sluice-fakes/src/lib.rs`
  and `crates/sluice/tests/profiles/conformance.rs`: `merge_base`
  beside `contains`.
- `crates/sluice/src/profiles/moon.rs`: `plan` reads the changed files
  once and hands them to moon's affected reading on its standard input.
- `crates/sluice-core/src/verdict.rs`: the replay proof as one function
  a combination's run and a worktree's run both read.
- `crates/sluice-core/src/session.rs`: the request's `since`, `beyond`
  and `replay`; the replays a finished run leaves unproven.
- `crates/sluice/src/exec.rs`: the opening line, the run again of the
  unproven replays, the summary and `--json`.
- `crates/sluice-core/src/step.rs`, `preview.rs`,
  `crates/sluice/src/profiles/checks.rs`: `beyond` empty.
- Tests: `crates/sluice/tests/exec.rs`,
  `crates/sluice/tests/profiles/moon.rs`, `sluice-core`'s unit tests.
- Pages: `site/src/content/docs/reference/commands.md`, `README.md`,
  `plugins/sluice/skills/using-sluice/references/exec.md`,
  `plugins/sluice/skills/using-sluice/SKILL.md`.
- Records and rules: a new record in `docs/adr/`, and rule core.9's
  details.
- Sources: clauses 16, 36, 70, 92, 93, 108, 138, 140, C.2;
  session-runs-1 clauses 89 and 92; records 0004, 0015, 0035; ops' RCA
  of 2026-11-18, cause 5.

## Touches

```
crates/sluice-seams/src/lib.rs
crates/sluice-fakes/src/lib.rs
crates/sluice-core/src/verdict.rs
crates/sluice-core/src/session.rs
crates/sluice-core/src/step.rs
crates/sluice-core/src/preview.rs
crates/sluice/src/exec.rs
crates/sluice/src/profiles/moon.rs
crates/sluice/src/profiles/checks.rs
crates/sluice/src/profiles/local_refs.rs
crates/sluice/src/profiles/pushed_refs.rs
crates/sluice/src/profiles/branches.rs
crates/sluice/src/profiles/github/pulls.rs
crates/sluice/tests/profiles/conformance.rs
crates/sluice/tests/exec.rs
crates/sluice/tests/profiles/moon.rs
site/src/content/docs/reference/commands.md
README.md
plugins/sluice/skills/using-sluice/references/exec.md
plugins/sluice/skills/using-sluice/SKILL.md
docs/adr/**
docs/architecture/core.md
openspec/changes/a-worktrees-checks-run-only-what-its-own-changes-affect/**
```
