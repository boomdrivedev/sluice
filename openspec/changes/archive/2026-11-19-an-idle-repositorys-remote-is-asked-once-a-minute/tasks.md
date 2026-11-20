# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0093 with clause 129 as amended 2026-11-19 and the code stage's choices, rule core.31's rule and details, and the reference's one-shot reads. True when `openspec validate --strict` passes on the specs and this change.

## 2. An idle repository's remote asked once a minute

- [x] 2.1 In `crates/sluice/src/host_process/tasks.rs`, mark each repository's last ask, the last reading's finding and an ask due, decide as each reading begins whether it asks, take the look only where an ask is due, and have a command's change, a watch's opening, a forecast's second ask and a binding made afresh ask at once. True when `cargo test -p sluice --lib host_process::tasks` passes, `an_idle_repositorys_remote_is_asked_once_a_minute` and `a_tasks_pass_asks_where_work_is_in_play_or_a_watch_opens` among it.
- [x] 2.2 Have `pass::pass` and `index::in_play` ask each remote only where the reading asks, the look holding its binding otherwise. True when `sluice exec main -- cargo test -p sluice --test host_process an_idle_repositorys_remote_is_asked_once_a_minute` passes.
- [x] 2.3 Have the daemon's peer test submit before the daemon starts, so its first ask finds the submission. True when `sluice exec main -- cargo test -p sluice --test peers a_daemon_lands_what_another_host_submits` passes.
