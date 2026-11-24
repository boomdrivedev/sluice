# Tasks

Work in this change's place on its branch, cut from `main`, the base
every check below runs against. Run suites through sluice, as
CLAUDE.md's Running checks has it; a quick `cargo check` or one unit
test may run directly. Commit by pathspec, Conventional Commits,
scoped to the part of the tree, citing clauses by number and rules by
id. Each group's time is measured as design.md's Measurements were,
and written into design.md's table of expected gate time.

## 1. A project's own `moon.yml` affects that project's checks

- [ ] 1.1 In `crates/sluice/src/profiles/moon.rs`, make `own_setting` name `.moon/`, the toolchain's files it names today and `.config/sluice.toml`, and no `moon.yml` (design D1). `Moon::plan` runs every required check where a changed file is an own setting, in place of its `graph_changed`. A changed `<dir>/moon.yml` adds every task of the project whose source is `<dir>` to the affected tasks, and every task that depends on one of them at any depth, read from the `deps` `moon query tasks` gives. True when `sluice exec main -- cargo test -p sluice --features tools --test moon a_projects_moon_yml_runs_its_project_and_its_dependents` passes. That new test is moon-runner's "A project's moon.yml".
- [ ] 1.2 The workspace's settings and the branch's policy run every check. True when `sluice exec main -- cargo test -p sluice --features tools --test moon the_workspaces_settings_and_the_policy_run_every_check` passes. That new test holds moon-runner's "The workspace's settings" and "The branch's policy".
- [ ] 1.3 `Moon::reads` reads the same way. True when `sluice exec main -- cargo test -p sluice --features tools --test moon a_moon_yml_no_check_reads_lands_beside` passes. That new test is moon-runner's "Work beside the chain".
- [ ] 1.4 A worktree's run reads a project's `moon.yml` as a combination's does. In `crates/sluice/tests/exec.rs`, `a_worktrees_own_moon_yml_runs_every_check` becomes `a_worktrees_own_moon_yml_runs_its_project`, and a new test holds exec-checks' "The toolchain". True when `sluice exec main -- cargo test -p sluice --features tools --test exec a_worktrees_own_moon_yml_runs_its_project` and `... --test exec a_worktrees_toolchain_runs_every_check` pass.
- [ ] 1.5 A record in `docs/adr/` for design D1, with Before and Now, and rule core.39's details and core.15's where they name a `moon.yml`. True when `sluice exec main -- moon run root:format` passes.

## 2. The herdr suite waits on events

- [ ] 2.1 In `plugins/herdr/tests/run.sh`, `within` and `ends_within` check every tenth of a second, and `reap` waits for each watcher it ends to be gone, with no sleep (design D2). True when `sluice exec main -- moon run plugin-herdr:test-sh` passes.
- [ ] 2.2 Under `SLUICE_HERDR_TEST`, `plugins/herdr/bin/watch.sh` writes its tick count to `$S/ticked` at the end of each pass; `settled` waits for the count to move by two, and a test's `sleep <n>` over idle ticks waits for that many ticks by the count. True when `sluice exec main -- moon run plugin-herdr:test-sh` passes.
- [ ] 2.3 The tests that count ticks run with `SLUICE_HERDR_TICK=0.2`. True when `sluice exec main -- moon run plugin-herdr:test-sh` passes and its run, timed as design.md's Measurements were, takes under 150 seconds of test time.

## 3. The herdr suite runs in four groups

- [ ] 3.1 `plugins/herdr/tests/run.sh` takes `--group <n>/<of>` and runs its own share, in a scratch folder of its own (design D3). `plugins/herdr/tests/groups.sh` runs four at once under the shell it is given and fails where any fails. `plugins/herdr/moon.yml`'s `test-sh` and `test-dash` run it under `tests/leaks.sh`. True when `sluice exec main -- moon run plugin-herdr:test-sh` and `... plugin-herdr:test-dash` pass, each within 90 seconds on a host at rest.
- [ ] 3.2 A failing test in one group fails the task and the other groups run to their end. True when `sluice exec main -- moon run root:leaks-cases` passes with a case for `groups.sh` beside its cases for `leaks.sh`.

## 4. No tools test is a chain of scenarios

- [ ] 4.1 In `crates/sluice/tests/profiles/pull_requests.rs`, `step_until` and `of_after`'s steps wait 50 milliseconds between steps; in `crates/sluice/tests/peers.rs` and `crates/sluice/tests/remote_branch.rs`, their loops the same (design D5). True when `sluice exec main -- cargo test -p sluice --features tools --test pull_requests` passes.
- [ ] 4.2 The check script's `slow` waits for the file the test writes once it has seen the run start, bounded by 5 seconds. True when `sluice exec main -- cargo test -p sluice --features tools --test pull_requests slow` passes.
- [ ] 4.3 Each test of `cli:tools` that runs scenarios one after another is split, one test for each, starting with every test design.md lists over 60 seconds alone. True when `sluice exec main -- moon run cli:tools` passes and its run, timed as design.md's Measurements were, has no test over 60 seconds on a host at rest.
- [ ] 4.4 `run_again_marks_a_draft_ready` waits on the state it needs, not on a count of rounds. True when `sluice exec main -- cargo test -p sluice --features tools --test pull_requests run_again_marks_a_draft_ready` passes.
- [ ] 4.5 nextest's threads for `cli:tools`, as design D5 point 4 decides, in `crates/sluice/moon.yml`. True when `sluice exec main -- moon run cli:tools` passes.

## 5. moon's results are kept for a replay

- [ ] 5.1 Find whether a combination's run can write moon's archives and shared store under the sandbox, by the run's log and its record (design D7). Where it cannot, give the run the store as rule core.35 asks, with a test in `crates/sluice/tests/checks.rs`. True when `sluice exec main -- cargo test -p sluice --features tools --test checks a_sandboxed_run_keeps_moons_archives` passes, or when design D7 records that the run already keeps them.
