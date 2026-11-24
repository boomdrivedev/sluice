# Tasks

Work in this change's place on its branch, cut from `main`, the base
every check below runs against. Run suites through sluice, as
CLAUDE.md's Running checks has it; a quick `cargo check` or one unit
test may run directly. Commit by pathspec, Conventional Commits,
scoped to the part of the tree, citing clauses by number and rules by
id. Each group's time is measured as design.md's Measurements were,
and written into design.md's table of expected gate time.

## 1. A project's own `moon.yml` affects that project's checks

- [x] 1.1 In `crates/sluice/src/profiles/moon.rs`, make `own_setting` name `.moon/`, the toolchain's files it names today and `.config/sluice.toml`, and no `moon.yml` (design D1). `Moon::plan` runs every required check where a changed file is an own setting, in place of its `graph_changed`. A changed `<dir>/moon.yml` adds every task of the project whose source is `<dir>` to the affected tasks, and every task that depends on one of them at any depth, read from the `deps` `moon query tasks` gives. True when `sluice exec main -- cargo test -p sluice --features tools --test moon a_projects_moon_yml_runs_its_project_and_its_dependents` passes. That new test is moon-runner's "A project's moon.yml".
- [x] 1.2 The workspace's settings and the branch's policy run every check. True when `sluice exec main -- cargo test -p sluice --features tools --test moon the_workspaces_settings_and_the_policy_run_every_check` passes. That new test holds moon-runner's "The workspace's settings" and "The branch's policy".
- [x] 1.3 `Moon::reads` reads the same way. True when `sluice exec main -- cargo test -p sluice --features tools --test moon a_moon_yml_no_check_reads_lands_beside` passes. That new test is moon-runner's "Work beside the chain".
- [x] 1.4 A worktree's run reads a project's `moon.yml` as a combination's does. In `crates/sluice/tests/exec.rs`, `a_worktrees_own_moon_yml_runs_every_check` becomes `a_worktrees_own_moon_yml_runs_its_project`, and a new test holds exec-checks' "The toolchain". True when `sluice exec main -- cargo test -p sluice --features tools --test exec a_worktrees_own_moon_yml_runs_its_project` and `... --test exec a_worktrees_toolchain_runs_every_check` pass.
- [x] 1.5 Design D1 proposed as the Boom decision `a-projects-own-moon-yml-affects-that-projects-checks@07db0a2d6188`, with Before and Now; no rule names a `moon.yml`. True when `boom show a-projects-own-moon-yml-affects-that-projects-checks` gives it.

## 2. The herdr suite waits on events

- [x] 2.1 In `plugins/herdr/tests/run.sh`, `within` and `ends_within` check every tenth of a second, and `reap` waits for each watcher it ends to be gone, with no sleep (design D2). True when `sluice exec main -- moon run plugin-herdr:test-sh` passes.
- [x] 2.2 `settled` checks every tenth of a second for three still seconds, and a test that kills a watcher waits for its source's identity to settle. True when `sluice exec main -- moon run plugin-herdr:test-sh` passes.

## 3. The herdr suite runs in four groups

- [x] 3.1 `plugins/herdr/tests/run.sh` takes `--group <n>/<of>` and runs its own share, in a scratch folder of its own (design D3). `plugins/herdr/tests/groups.sh` runs four at once under the shell it is given and fails where any fails. `plugins/herdr/moon.yml`'s `test-sh` and `test-dash` run it under `tests/leaks.sh`. True when `sluice exec main -- moon run plugin-herdr:test-sh plugin-herdr:test-dash` passes.
- [x] 3.2 A definition one test made and later tests read stands between tests, so every group holds it. True when `sluice exec main -- moon run plugin-herdr:test-sh` passes with every group's tests.

## 4. No tools test is a chain of scenarios

- [x] 4.1 In `crates/sluice/tests/profiles/pull_requests.rs`, `step_until` and the other loops wait 50 milliseconds between steps; in `crates/sluice/tests/peers.rs` and `crates/sluice/tests/remote_branch.rs`, their loops the same (design D5). True when `sluice exec main -- moon run cli:tools` passes.
- [x] 4.2 `s56_a_refused_request_waits_for_its_submitter` is three tests, one for each scenario. True when `sluice exec main -- cargo nextest run -p sluice --features tools --test pull_requests -E 'test(s56)'` passes.
- [x] 4.3 Count the processes one `sluice step` of the `pull_requests` fixture starts, the `git` calls among them, and where its time goes, and write it into design D6. True when design D6 gives them, measured.

- [x] 4.4 A round fetches its branch's state once from each remote, reading what it fetched or wrote from then on, and fetches again after a refused write (design D6). True when `sluice exec main -- cargo nextest run -p sluice --features tools --test remote_refs -E 'test(a_round_fetches_the_state_once)'` passes.

## 5. moon's results are kept for a replay

- [x] 5.1 Find whether a combination's run can write moon's shared store under the sandbox (design D9). True when design D9 records it.
