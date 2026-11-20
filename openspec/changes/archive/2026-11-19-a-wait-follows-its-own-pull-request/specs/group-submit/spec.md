## ADDED Requirements

### Requirement: Under pull requests each request names the wait that follows it

Under pull requests, the act's answer SHALL name, for each pull request
still open and undecided, the wait that follows it, `sluice wait
<branch> --pull-request <n>`: under `--json` as `wait` in its item of
`submissions`, beside `pull_request`, and in words on a line `wait:
sluice wait <branch> --pull-request <n>`. A waiting act that ends with
one still open SHALL say `still open: #<n>; sluice wait <branch>
--pull-request <n>` for it (125, C.5).

#### Scenario: No wait, every condition holding

- **WHEN** `main` takes pull requests with no conditions, and `sluice
  submit main --worktree ../b1 --worktree ../b2 --no-wait --json` runs,
  opening `<n1>` and `<n2>`
- **THEN** it exits 0, and each item of `submissions` holds `id`,
  `pull_request` and `wait`, `sluice wait main --pull-request <n1>` for
  the first and `sluice wait main --pull-request <n2>` for the second

#### Scenario: No wait, in words

- **WHEN** the same act runs without `--json`
- **THEN** its output holds `wait: sluice wait main --pull-request <n1>`
  and `wait: sluice wait main --pull-request <n2>`

#### Scenario: Stopped while both are open

- **WHEN** the same act runs without `--no-wait`, and is stopped with
  Ctrl-C before either pull request is decided
- **THEN** it says `still open: #<n1>; sluice wait main --pull-request
  <n1>` and `still open: #<n2>; sluice wait main --pull-request <n2>`,
  and exits 130
