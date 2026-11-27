# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. The submit's answer gives its wait

- [x] 1.1 In `submit` and `submit_group` in `crates/sluice/src/lib.rs`, add `wait`, `sluice wait <id> --branch <branch>`, to the answer under `--no-wait --json`, and its line to the answer in words under `--no-wait` (D1). Check: `sluice exec main -- cargo test -p sluice --test cli a_submits_answer_gives_the_wait_that_names_its_branch`.
- [x] 1.2 Update the tests that compare the whole answer: in `crates/sluice/tests/cli.rs`, `remote_branch.rs` and `profiles/worktrunk.rs`. Check: `sluice exec main -- moon run sluice:test`.

## 2. The worktree's own submission

- [x] 2.1 Add `submitted_here` and `lookup_here` to `crates/sluice/src/lib.rs`, which read the worktree's own submission by `Sluice::made`, and use them in `wait`, `status`, `amend` and `withdraw` where more than one branch holds the number and no `--branch` is given (D2). Check: `sluice exec main -- cargo test -p sluice --test cli number_on_two_branches`.
- [x] 2.2 In `crates/sluice/src/host_process/reads.rs` and `changes.rs`, run a status, an amend and a withdrawal with the request's folder as the worktree (D3). Check: `sluice exec main -- cargo test -p sluice --lib runs_as_from`.

## 3. The record, the claim and the skill

- [x] 3.1 Amend record 0114 for 2026-11-26 in `docs/adr/`.
- [x] 3.2 Propose a new version of the claim `a-number-two-branches-hold-names-neither-alone` in Boom, with the worktree's own submission as a case that does not break it.
- [x] 3.3 Give the submit's `wait` and the worktree's own submission in `plugins/sluice/skills/using-sluice/SKILL.md` and `references/submit.md`.

## 4. The change

- [x] 4.1 Run `openspec validate a-submission-is-named-by-its-branch --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 4.2 Archive the change with `openspec archive a-submission-is-named-by-its-branch`, so `openspec/specs/number-lookup/spec.md` holds its requirements.
