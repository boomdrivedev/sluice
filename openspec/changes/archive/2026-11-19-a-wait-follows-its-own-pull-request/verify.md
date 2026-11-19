# Verify: a-wait-follows-its-own-pull-request

Verified on `change/a-wait-follows-its-own-pull-request` at 822b2ee,
cut from `bolt/sluice-checks-and-waits-on-an-agents-own-work` at
13bfe44, on 2026-11-19.

## The whole checks

`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work`, the
bolt's gate `#check-fast`, run once at 822b2ee: exit 0, `passed: 6
tasks`, `recorded 6 results under their keys for 822b2ee`.

| Task | Outcome |
|---|---|
| `plugin-sluice:validate` | passed |
| `nix:typecheck` | passed |
| `root:lint` | passed |
| `root:changes` | passed |
| `root:format` | passed |
| `cli:test` | passed, the four new cli tests among it |

The bolt's gate runs no `--features tools` test, so the change's tests
in `pull_requests` were run once more through sluice, as its tasks name
them:
`sluice exec bolt/sluice-checks-and-waits-on-an-agents-own-work -- cargo
test -p sluice --features tools --test pull_requests -- --exact <19
names>`, exit 0, `19 passed; 0 failed`. The 19 are the 16 the tasks name
and the three existing tests the tasks change to read `wait`:
`the_status_a_submit_reads_across_hosts`,
`run_again_follows_a_pull_request_waiting_for_a_review` and
`run_again_marks_ready_a_draft_another_host_sent_back`. `#check-tools`
runs the whole binary when the bolt lands on main.

## OpenSpec

- `openspec instructions apply --change a-wait-follows-its-own-pull-request --json`:
  `state: all_done`, 24 of 24 tasks done.
- `openspec validate a-wait-follows-its-own-pull-request --strict`:
  `Change 'a-wait-follows-its-own-pull-request' is valid`.
- `root:changes`, which holds the change to archiving alone against the
  tree's specs (rule core.61), passed in the gate.

## Requirements and scenarios

### submission-wait

| Requirement | Scenario | Test | Result |
|---|---|---|---|
| A wait names a pull request by its number | Held on another host, out of time | `a_wait_by_pull_request_from_the_main_checkout_follows_it_on_another_host` | pass |
| | Landed on another host | the same | pass |
| | Taken in here, out of time | `a_wait_by_pull_request_taken_in_here_prints_its_record` | pass |
| | Another worktree's pull request | `a_wait_by_another_worktrees_pull_request_hands_nothing_over` | pass |
| A wait refuses a pull request it cannot follow | With a submission's number | `a_wait_by_pull_request_with_a_number_exits_64` (cli) | pass |
| | Into another branch | `a_wait_by_pull_request_into_another_branch_is_not_found` | pass |
| | A branch that takes no pull requests | the same | pass |
| An undecided pull request's document names its wait | Out of time, held on another host | `out_of_time_queued_on_the_other_host` | pass |
| | Out of time, waiting for a review, in words | `out_of_time_waiting_for_a_review_names_its_wait` | pass |
| | Taken in here, out of time | `a_wait_on_a_request_taken_in_here_out_of_time_prints_its_record` | pass |
| A wait that could not read its pull request names itself again | The forge never answers | `a_wait_by_pull_request_that_could_not_read_names_itself_again` (cli), with `a_pull_requests_wait_that_could_not_read_names_itself_again` and `following_a_pull_request_the_forge_could_not_answer_says_it_could_not_read` for the wait from the worktree | pass |

### pull-request-submit

| Requirement | Scenario | Test | Result |
|---|---|---|---|
| A submit that does not wait answers in one document (modified) | No wait, its conditions hold | `no_wait_answers_with_its_submission` | pass |
| | No wait, waiting for a review | `no_wait_waiting_for_a_review_answers_with_the_pull_request` | pass |
| A pull request's answer names the wait that follows it | No wait, another host takes it in | `no_wait_on_a_host_that_does_not_take_it_in_names_its_wait` | pass |
| | No wait, in words, taken in | `no_wait_says_its_submission` | pass |
| | No wait, in words, waiting for a review | `no_wait_waiting_for_a_review_names_its_wait_in_words` | pass |
| | Its status | `status_names_the_head_waiting_for_a_review` | pass |
| | Submitted by name | `a_submit_by_name_names_its_pull_requests_wait` | pass |

### group-submit

| Requirement | Scenario | Test | Result |
|---|---|---|---|
| Under pull requests each request names the wait that follows it | No wait, every condition holding | `a_group_act_names_each_pull_requests_wait` | pass |
| | No wait, in words | the same | pass |
| | Stopped while both are open | `a_group_act_stopped_names_each_open_pull_requests_wait` | pass |

### Documents and records

- `sluice wait --help` gives the example and both exit lines;
  `wait_help_names_a_wait_by_pull_request` passes.
- The commands reference, the pull requests guide, the using-sluice
  skill and its submit reference give `wait` in the `--no-wait`
  documents, the `still open` line with the wait by pull request, and
  `queue.submission` as the other host's number. The added lines name
  no host or tool of ours (rule core.26).
- Record 0130 (`docs/adr/0130-a-pull-requests-wait-is-named-by-its-number.md`)
  holds D1 and D2 with their alternatives, and rule core.48's details
  rule out an answer that names no wait and a way on that reads the
  folder it runs in, with the signal's case, citing the record.

## Findings

- `crates/sluice/src/lib.rs:5296`: the constant
  `PULL_REQUEST_GOES_WITH_A_BRANCH` sits between
  `branch_goes_with_a_number`'s doc comment and that function. The
  constant takes the function's doc comment as its own, and the function
  is left with none. This is cosmetic: no requirement or rule reads it,
  and the work holds without it.

## Outcome

The whole checks pass, every task is done, the change validates
strictly, and every scenario of its three specs is met by a test that
passes. The change is archived.
