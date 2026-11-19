# Verify: a-worktrees-checks-run-only-what-its-own-changes-affect

Verified on 2026-11-19 at `e2b68d7`, on
`change/a-worktrees-checks-run-only-what-its-own-changes-affect`, cut
from `bolt/sluice-checks-and-waits-on-an-agents-own-work` at `13bfe44`.

**Outcome: not archived.** The bolt's gate fails on `cli:test`: an
existing test of the moon runner fails against this change's moon
profile. The change goes back to its code stage.

## The whole checks

`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work`
(sluice 0.95.1, the bolt's gate `#check-fast`), run once, exited 1 after
8m 24s: `Tasks: 9 completed, 1 failed`.

| Task | Outcome |
|---|---|
| `plugin-sluice:validate` | passed |
| `nix:typecheck` | passed |
| `seams:test` | passed, 5 tests |
| `fakes:test` | passed, 17 tests |
| `root:changes` | passed, 6 open changes valid and archiving |
| `root:format` | passed |
| `root:lint` | passed |
| `core:test` | passed, 172 tests, `a_worktrees_replay_of_a_flaky_key_reads_as_the_branch_says` among them |
| `conformance:test` | passed |
| `cli:test` | **failed** |

`cli:test` stopped at `tests/checks.rs`:

```
---- a_run_that_runs_nothing_is_never_set_up stdout ----
thread 'a_run_that_runs_nothing_is_never_set_up' panicked at crates/sluice/tests/checks.rs:1801:5:
["toolchain", "proto install --config-mode upwards", "moon --version", "moon query tasks"]

test result: FAILED. 49 passed; 1 failed; 0 ignored; 0 measured; 9 filtered out; finished in 9.73s
error: test failed, to rerun pass `-p sluice --test checks`
```

The test plans a moon request whose `since` is its own revision and
asserts that the plan asks `moon query tasks --affected`. Task 2.1's
`Moon::tasks` now returns at once for an empty list of changed files
(`Affected::Files([]) => return Ok(BTreeMap::new())` in
`crates/sluice/src/profiles/moon.rs`), so moon is never asked and the
assertion at line 1801 fails. Either the shortcut or the test has to
change; which one is the code stage's to decide, saying so in the
design's D3.

Since cargo stops at the first test binary that fails, the binaries
after `tests/checks.rs` did not run in this gate. Those include the
change's own tests in `tests/exec.rs`, `tests/local_refs.rs` and
`tests/profiles/moon.rs`.

sluice read the run as follows:

```
could not run: cli:test: sluice's sandbox refused the run file-write-create /Users/chuck/.local/share/boomdrive/repositories/boomdrivedev/sluice/sluice.git/moon-outputs/d460a93d493621cbebb7b571a1ec4b6bed617a4e383d60b81173f2b3a1aad0ab.tar.gz (by moon), network-outbound remote:*:53 (by moon), network-outbound /private/var/run/mDNSResponder (a name lookup, by moon) and 15 more
nothing was recorded for a test that could not run: once the host can run it, run again
recorded 9 results under their keys for e2b68d7
```

The sandbox's refusals of moon's output archive and name lookups turned
the fail into "could not run" (rule core.35). The fail itself does not
depend on them: the test's own log shows moon was never asked the
affected query. The refusals are a separate matter for the host. moon
also warned in `root:changes`, `Failed to package task outputs into
archive`.

## OpenSpec

- `openspec instructions apply --change a-worktrees-checks-run-only-what-its-own-changes-affect --json`:
  `state: all_done`, 18 of 18 tasks complete.
- `openspec validate a-worktrees-checks-run-only-what-its-own-changes-affect --strict`:
  `Change 'a-worktrees-checks-run-only-what-its-own-changes-affect' is valid`, exit 0.

## The specs against the work

The `exec-checks` delta spec, by requirement. Each scenario has a test
written for it and code that does what the test asserts. Those in
`tests/exec.rs` and `tests/local_refs.rs` did not run in this gate (see
above).

| Requirement | Scenario | Test | Code |
|---|---|---|---|
| A worktree's checks are those its own changes affect | The branch moved on | `a_worktrees_checks_leave_out_what_its_branch_landed_since` | `Sluice::session_since`, merge base by `Branch::merge_base` |
| | An edit not committed | `a_worktrees_unstaged_edit_runs_the_checks_it_affects` | `SessionWork::checks` reads `session.dirty` into `beyond` |
| | A file not yet tracked | `a_worktrees_untracked_file_runs_the_checks_it_affects` | the same, `??` lines through `path_of` |
| | Nothing of its own | `a_worktree_at_its_branchs_revision_runs_no_check` | `Affected::Files([])` returns no task |
| A change to the runner's own settings runs every check | Its own moon.yml | `a_worktrees_own_moon_yml_runs_every_check` | `Moon::plan`'s check for `.moon/` and `moon.yml` over changed files and `beyond` |
| | No history in common | `exec_opens_with_where_the_worktree_left_its_branch` | `since: None` runs every required check; `opening` says why |
| | Script runner, every check (no scenario) | none | only the moon profile reads `since` or `beyond` |
| A check sluice already passed on the same files is replayed | The same files again | `a_second_run_on_the_same_files_replays_and_records_nothing` | `replay: true` for `SessionWork::Checks`; `read_replays` through `verdict::read` |
| | After a fix | `after_a_fix_exec_runs_the_fixed_check_and_replays_the_rest` | the same |
| | A counted fail too reads as `flaky` says (no scenario) | `a_worktrees_replay_of_a_flaky_key_reads_as_the_branch_says` (passed in `core:test`) | `read_replays` marks a replay `verdict::read` fails |
| A replay sluice holds no pass for runs again | A pass sluice never recorded | `a_pass_sluice_never_recorded_runs_again_and_is_recorded` | `SessionWork::Again`, `finish_again`, `joined`, `joined_recorded` |
| The output says what was read and what was replayed | A worktree behind its branch | `exec_opens_with_where_the_worktree_left_its_branch` | `opening` in `exec.rs` |
| | The document for scripts | `exec_json_gives_since_and_each_tasks_replayed` | `since` and `replayed` in `--json` |

The documentation tasks (3.7, 4.4, 5.3, 5.4) hold on reading:

- `README.md` and `site/src/content/docs/reference/commands.md` say a
  worktree's run reads its own changes since it left the branch, its
  uncommitted and untracked files included. No example opens with
  `running the checks <head> affects against`.
- Record 0130 holds D1, D4 and D5 with their alternatives and the named
  sources. Rule core.9's details carry its two clauses.
- The skill and its `exec.md` reference drop "forced past moon's cache"
  and `--force`.

The work does what the spec asks. The gate does not pass because of the
regression above.
