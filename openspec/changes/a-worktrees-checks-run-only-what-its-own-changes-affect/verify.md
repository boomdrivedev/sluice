# Verify: a-worktrees-checks-run-only-what-its-own-changes-affect

Verified on 2026-11-19 at `48c0112`, on
`change/a-worktrees-checks-run-only-what-its-own-changes-affect`, cut
from `bolt/sluice-checks-and-waits-on-an-agents-own-work` at `13bfe44`.

**Outcome: not archived.** The bolt's gate fails on `cli:test`: an
existing test of `sluice exec` expects a second run on the same files to
fail, and this change's replay answers it with the first run's recorded
pass. The change goes back to its code stage.

## The whole checks

`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work`
(sluice 0.95.1, the bolt's gate `#check-fast`), run once, exited 1 after
4m 10s: `Tasks: 9 completed, 1 failed`.

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
| `cli:test` | **failed** |

`cli:test` (`cargo test -p sluice -- --skip sandbox`) passed the library's
332 unit tests and `tests/checks.rs`'s 50, the code stage's
`a_run_that_runs_nothing_is_never_set_up` among them. It stopped at
`tests/cli.rs`:

```
---- exec_runs_the_affected_checks_against_the_branch stdout ----

thread 'exec_runs_the_affected_checks_against_the_branch' (2437317850) panicked at crates/sluice/tests/cli.rs:4607:5:
assertion `left == right` failed
  left: 0
  right: 1

failures:
    exec_runs_the_affected_checks_against_the_branch

test result: FAILED. 241 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 183.29s

error: test failed, to rerun pass `-p sluice --test cli`
```

The test's first half passes as this change rewrote it: the request asks
`replay`, `since` is the merge base, and the opening line is D6's. Its
second half is as it was before the change:

```rust
f.world
    .fails_with("plugin:test", "plugin/lib/plan.py", "p2");
let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
assert_eq!(code, FAILED);
```

The worktree's files are the same as on the first run, which passed
`plugin:test` and recorded it. With `replay: true` the fake replays a
key it holds a pass for (`crates/sluice-fakes/src/lib.rs`, the task run
"replayed where the run may and a pass" is held), so the fail rule never
runs. The replay is proven by the recorded pass, and exec exits 0. That
is what the spec's "The same files again" asks. The test's second half
still expects a forced run, so it, or how it makes the check fail, has to
change. Which one is the code stage's to decide.

Since cargo stops at the first test binary that fails, the binaries
after `tests/cli.rs` did not run in this gate. The change's own tests in
`tests/exec.rs` and `tests/local_refs.rs` are `#![cfg(feature = "tools")]`
and `tests/profiles/moon.rs` is moon's. None is in the bolt's
`#check-fast`; each runs in main's `#check-tools`.

sluice read the run as follows:

```
could not run: cli:test: sluice's sandbox refused the run file-write-create /Users/chuck/.local/share/boomdrive/repositories/boomdrivedev/sluice/sluice.git/moon-outputs/d460a93d493621cbebb7b571a1ec4b6bed617a4e383d60b81173f2b3a1aad0ab.tar.gz (by moon), network-outbound remote:*:53 (by moon), network-outbound /private/var/run/mDNSResponder (a name lookup, by moon) and 15 more
nothing was recorded for a test that could not run: once the host can run it, run again
recorded 9 results under their keys for 48c0112
```

The sandbox's refusals of moon's output archive and name lookups turned
the fail into "could not run" (rule core.35). The fail does not depend
on them: the test's assertion reads the exit of a fake world's exec. The
refusals are a separate matter for the host.

## OpenSpec

- `openspec instructions apply --change a-worktrees-checks-run-only-what-its-own-changes-affect --json`:
  `state: all_done`, 19 of 19 tasks complete.
- `openspec validate a-worktrees-checks-run-only-what-its-own-changes-affect --strict`:
  `Change 'a-worktrees-checks-run-only-what-its-own-changes-affect' is valid`, exit 0.

## The specs against the work

The `exec-checks` delta spec, by requirement. Each scenario has a test
written for it and code that does what the test asserts. Those in
`tests/exec.rs` and `tests/local_refs.rs` are main's `#check-tools` and
did not run in this gate.

| Requirement | Scenario | Test | Code |
|---|---|---|---|
| A worktree's checks are those its own changes affect | The branch moved on | `a_worktrees_checks_leave_out_what_its_branch_landed_since` | `Sluice::session_since`, merge base by `Branch::merge_base` |
| | An edit not committed | `a_worktrees_unstaged_edit_runs_the_checks_it_affects` | `SessionWork::checks` reads `session.dirty` into `beyond` |
| | A file not yet tracked | `a_worktrees_untracked_file_runs_the_checks_it_affects` | the same, `??` lines through `path_of` |
| | Nothing of its own | `a_worktree_at_its_branchs_revision_runs_no_check` | `Affected::Files([])` asks moon nothing and returns no task (D3) |
| A change to the runner's own settings runs every check | Its own moon.yml | `a_worktrees_own_moon_yml_runs_every_check` | `Moon::plan`'s check for `.moon/` and `moon.yml` over changed files and `beyond` |
| | No history in common | `exec_opens_with_where_the_worktree_left_its_branch` | `since: None` runs every required check; `opening` says why |
| | Script runner, every check (no scenario) | none | only the moon profile reads `since` or `beyond` |
| A check sluice already passed on the same files is replayed | The same files again | `a_second_run_on_the_same_files_replays_and_records_nothing` | `replay: true` for `SessionWork::Checks`; `read_replays` through `verdict::read` |
| | After a fix | `after_a_fix_exec_runs_the_fixed_check_and_replays_the_rest` | the same |
| | A counted fail too reads as `flaky` says (no scenario) | `a_worktrees_replay_of_a_flaky_key_reads_as_the_branch_says` (passed in `core:test`) | `read_replays` marks a replay `verdict::read` fails |
| A replay sluice holds no pass for runs again | A pass sluice never recorded | `a_pass_sluice_never_recorded_runs_again_and_is_recorded` | `SessionWork::Again`, `finish_again`, `joined`, `joined_recorded` |
| The output says what was read and what was replayed | A worktree behind its branch | `exec_opens_with_where_the_worktree_left_its_branch` | `opening` in `exec.rs` |
| | The document for scripts | `exec_json_gives_since_and_each_tasks_replayed` | `since` and `replayed` in `--json` |

The design's D3 now says an empty list of changed files asks moon
nothing, and `a_run_that_runs_nothing_is_never_set_up` holds both forms
of a run that runs nothing. It passed in this gate.

The documentation tasks (3.7, 4.4, 5.3, 5.4) hold on reading:

- `README.md` and `site/src/content/docs/reference/commands.md` say a
  worktree's run reads its own changes since it left the branch, its
  uncommitted and untracked files included. No example opens with
  `running the checks <head> affects against`.
- Record 0130 holds D1, D4 and D5 with their alternatives and the named
  sources. Rule core.9's details carry its two clauses.
- The skill and its `exec.md` reference drop "forced past moon's cache"
  and `--force`. The `--force` left in `commands.md` is `sluice init
  actions`' own.

The work does what the spec asks. The gate does not pass because of the
test above.
