## MODIFIED Requirements

### Requirement: A submit that does not wait answers in one document
Under `--json`, a `--no-wait` submit's answer with a submission SHALL be
the document a `--no-wait` submit prints on any branch, `id`, `branch`
and `ahead`, with `pull_request`, the pull request's number, and `wait`,
the wait that follows it, beside them; its answer with a pull request
SHALL be `pull_request`, `branch`, `state`, the state sluice posts on
it, and `wait`.

#### Scenario: No wait, its conditions hold
- **WHEN** `main` takes pull requests with no conditions, and an agent runs `wt submit main --no-wait --json` from a worktree on `unit/ready`
- **THEN** it exits 0 and prints one document with the `id` of a submission from the pull request's checkout `pull/<n>` at the head pushed, `branch` `main`, `ahead`, `pull_request` `<n>` and `wait` `sluice wait main --pull-request <n>`; and `sluice wait <id> --json` follows that submission to `landed`

#### Scenario: No wait, waiting for a review
- **WHEN** `main` takes pull requests with `reviews = 1`, and an agent runs `wt submit main --no-wait --json`
- **THEN** it exits 0 and prints `{"pull_request": <n>, "branch": "main", "state": "waiting: needs 1 review", "wait": "sluice wait main --pull-request <n>"}`, with no `id`, and no submission of the pull request is queued

## ADDED Requirements

### Requirement: A pull request's answer names the wait that follows it

Wherever a submit to a branch that takes pull requests answers for a
pull request still open and undecided, with `--no-wait` or `--status`,
from a worktree or by name (124), whichever host takes it in (137), its
answer SHALL name the wait that follows that request, `sluice wait
<branch> --pull-request <n>`: under `--json` as `wait`, and in words on
a line of its own after the answer, `wait: sluice wait <branch>
--pull-request <n>` (C.5; rule core.48).

#### Scenario: No wait, another host takes it in

- **WHEN** host `b` keeps `main` and takes its pull requests in, and an
  agent runs `sluice submit main --no-wait --json` on host `a` from a
  worktree on `unit/two-apps`, which opens pull request `<n>`
- **THEN** it exits 0 and prints `pull_request` `<n>`, `branch` `main`,
  `state`, `taken_in_by` `{"host":"b"}` and `wait` `sluice wait main
  --pull-request <n>`, with no `id`; and that wait, run in the
  repository's main checkout on `a`, follows `<n>` to the outcome `b`
  gives it

#### Scenario: No wait, in words, taken in

- **WHEN** `main` takes pull requests with no conditions, and an agent
  runs `wt submit main --no-wait` from a worktree on `unit/ready`
- **THEN** after `submission <id> to main from #<n> · nothing queued
  ahead` it prints `wait: sluice wait main --pull-request <n>`, and
  exits 0

#### Scenario: No wait, in words, waiting for a review

- **WHEN** `main` takes pull requests with `reviews = 1`, and an agent
  runs `wt submit main --no-wait` from a worktree on `unit/review`
- **THEN** after `waiting: needs 1 review` it prints `wait: sluice wait
  main --pull-request <n>`, and exits 0

#### Scenario: Its status

- **WHEN** an agent's pull request `<n>` waits for a review, and `wt
  submit main --status --json` runs in the agent's worktree
- **THEN** it prints `wait` `sluice wait main --pull-request <n>` beside
  `pull_request`, `branch`, `state`, `head`, `draft` and `sends`

#### Scenario: Submitted by name

- **WHEN** `main` takes pull requests with `reviews = 1`, and `sluice
  submit main --head bolt/x --no-wait --json` runs in the repository's
  main checkout, which opens pull request `<n>` from `bolt/x`
- **THEN** it prints `pull_request` `<n>` and `wait` `sluice wait main
  --pull-request <n>`, with no `id`
