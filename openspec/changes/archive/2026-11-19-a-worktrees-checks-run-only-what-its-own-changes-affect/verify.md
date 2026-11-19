# Verify: a-worktrees-checks-run-only-what-its-own-changes-affect

Verified on 2026-11-19 at `45507f3`, on
`change/a-worktrees-checks-run-only-what-its-own-changes-affect`, cut
from `bolt/sluice-checks-and-waits-on-an-agents-own-work` at `13bfe44`.

**Outcome: the whole checks pass, OpenSpec's validation passes, every
task is done and the work meets every requirement and scenario of the
change's spec. The change goes to OpenSpec's archive.**

## The whole checks

`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work`
(sluice 0.95.1, the bolt's gate `#check-fast`), run once, exited 0 after
7m 49s:

```
Tasks: 10 completed
passed: 10 tasks
recorded 10 results under their keys for 45507f3
```

| Task | Outcome |
|---|---|
| `plugin-sluice:validate` | passed |
| `nix:typecheck` | passed |
| `seams:test` | passed, 5 tests |
| `fakes:test` | passed, 17 tests |
| `root:changes` | passed |
| `root:format` | passed |
| `root:lint` | passed |
| `core:test` | passed, 172 tests, `a_worktrees_replay_of_a_flaky_key_reads_as_the_branch_says` among them |
| `conformance:test` | passed |
| `cli:test` | passed: the library's 332 unit tests, `tests/checks.rs`'s 50 (`a_run_that_runs_nothing_is_never_set_up` among them), `tests/cli.rs`'s 242 (`exec_runs_the_affected_checks_against_the_branch` among them) and every binary after it |

The run opened with `running the checks 45507f3 affects against
bolt/sluice-checks-and-waits-on-an-agents-own-work (13bfe44)`, the
installed sluice 0.95.1's words; this change's opening line is in the
binary it builds. moon warned twice that it could not package a task's
outputs into its archive under `.moon/cache/outputs`. That decided
nothing: each task passed and its result was recorded.

## The change's own tests

The scenario tests in `tests/exec.rs`, `tests/local_refs.rs` and
`tests/profiles/moon.rs` are `#![cfg(feature = "tools")]`, main's
`#check-tools` (`cli:tools`), and ran no test in the bolt's gate. They
were run once through sluice as `cli:tools` runs them, sandbox tests
left out, which macOS refuses inside sluice's sandbox (`crates/sluice/moon.yml`,
`cli:sandbox`):

`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work -- cargo
test -p sluice --features tools --test exec --test local_refs --test moon
-- --skip sandbox` exited 0.

| Binary | Outcome |
|---|---|
| `tests/exec.rs` | 29 passed, 3 sandbox tests left out |
| `tests/local_refs.rs` | 18 passed, `a_merge_base_is_where_two_revisions_part` among them |
| `tests/profiles/moon.rs` | 34 passed, 3 sandbox tests left out, `the_tasks_a_range_affects` and `a_plan_reads_the_paths_a_worktree_holds_beyond_its_revision` among them |

## OpenSpec

- `openspec instructions apply --change a-worktrees-checks-run-only-what-its-own-changes-affect --json`:
  `state: all_done`, 20 of 20 tasks complete.
- `openspec validate a-worktrees-checks-run-only-what-its-own-changes-affect --strict`:
  `Change 'a-worktrees-checks-run-only-what-its-own-changes-affect' is valid`, exit 0.

## The specs against the work

The `exec-checks` delta spec, by requirement. Every scenario's test
passed in one of the two runs above.

| Requirement | Scenario | Test | Code |
|---|---|---|---|
| A worktree's checks are those its own changes affect | The branch moved on | `a_worktrees_checks_leave_out_what_its_branch_landed_since` | `Sluice::session_since`, merge base by `Branch::merge_base` |
| | An edit not committed | `a_worktrees_unstaged_edit_runs_the_checks_it_affects` | `session_request` reads `session.dirty` into `beyond` |
| | A file not yet tracked | `a_worktrees_untracked_file_runs_the_checks_it_affects` | the same, untracked entries through the one path reader |
| | Nothing of its own | `a_worktree_at_its_branchs_revision_runs_no_check` | an empty list of changed files asks moon nothing and returns no task (D3) |
| A change to the runner's own settings runs every check | Its own moon.yml | `a_worktrees_own_moon_yml_runs_every_check` | `Moon::plan`'s check for `.moon/` and `moon.yml` over the changed files and `beyond` |
| | No history in common | `exec_opens_with_where_the_worktree_left_its_branch` | `since: None` runs every required check; the opening line says why |
| | Script runner, every check (no scenario) | none needed | only the moon profile reads `since` or `beyond` |
| A check sluice already passed on the same files is replayed | The same files again | `a_second_run_on_the_same_files_replays_and_records_nothing` | `replay: true` for a run of the checks; replays read through `verdict::read` |
| | After a fix | `after_a_fix_exec_runs_the_fixed_check_and_replays_the_rest` | the same |
| | A counted fail too reads as `flaky` says (no scenario) | `a_worktrees_replay_of_a_flaky_key_reads_as_the_branch_says` | a replay `verdict::read` fails is a failed check |
| A replay sluice holds no pass for runs again | A pass sluice never recorded | `a_pass_sluice_never_recorded_runs_again_and_is_recorded` | `replayed with no result sluice recorded: …; running them again`, the second run joined into the summary and the exit |
| The output says what was read and what was replayed | A worktree behind its branch | `exec_opens_with_where_the_worktree_left_its_branch` | the opening line in `exec.rs` |
| | The document for scripts | `exec_json_gives_since_and_each_tasks_replayed` | `since` and `replayed` in `--json` |

`exec_runs_the_affected_checks_against_the_branch` (task 5.5) makes its
second run's fail on new files, so the check runs and its fail is the
exit, and passed in `cli:test`.

The documentation tasks (3.7, 4.4, 5.3, 5.4) hold on reading:

- `README.md` and `site/src/content/docs/reference/commands.md` say a
  worktree's run reads its own changes since it left the branch, its
  uncommitted and untracked files included. No example opens with
  `running the checks <head> affects against`; the examples open with
  `running the checks the worktree's own changes affect since …`.
- Record 0131 holds D1, D4 and D5 with their alternatives. Rule core.9's
  details carry its two clauses, citing record 0131.
- The skill and its `exec.md` reference say a check sluice passed on the
  same files is replayed and a replay sluice holds no pass for runs
  again, and no longer say "forced past moon's cache". The commands page
  gives `replayed with no result sluice recorded: …; running them again`.
