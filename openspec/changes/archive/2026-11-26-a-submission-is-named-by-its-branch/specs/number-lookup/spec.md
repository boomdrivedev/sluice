## MODIFIED Requirements

### Requirement: A number more than one branch holds names none of them alone

A lookup by number SHALL read every branch it can. More than one branch
can hold the number. Then, unless the worktree that asks made it on one
of them only, sluice SHALL refuse the lookup and do nothing. The refusal
SHALL name each branch that holds it, in the records' order. It SHALL
also name each branch that could not be read, with its words (26, record
0114, rule core.11). This holds for `wait`, `status`, `amend` and `withdraw`.
A number that one branch holds SHALL be found as before.

#### Scenario: Withdraw refused naming both branches

- **GIVEN** submission 26 queued on `bolt/a` and submission 26 landed
  on `bolt/b`
- **WHEN** `sluice withdraw 26` runs
- **THEN** it says `refused: submission 26 is on more than one branch:
  bolt/a, bolt/b; name one with --branch, as sluice withdraw 26
  --branch bolt/a`, exits 2, and both submissions are as they were

#### Scenario: Status and amend refused the same way

- **GIVEN** submission 26 queued on `bolt/a` and on `bolt/b`
- **WHEN** `sluice status 26` and `sluice amend 26 --priority 5` run
- **THEN** each is refused naming `bolt/a` and `bolt/b` and exits 2, and
  neither submission's priority changes

#### Scenario: A branch that could not be read is named beside them

- **GIVEN** submission 26 on `bolt/a` and on `bolt/b`, and `bolt/c`'s
  records refused with the words `<words>`
- **WHEN** `sluice status 26` runs
- **THEN** it is refused naming `bolt/a` and `bolt/b`, then says
  `bolt/c could not be read: <words>`, and exits 2

#### Scenario: A number one branch holds

- **GIVEN** submission 27 queued on `bolt/a` alone, and submission 26 on
  `bolt/a` and on `bolt/b`
- **WHEN** `sluice withdraw 27` runs
- **THEN** it withdraws `bolt/a`'s 27 and exits 0, as before

## ADDED Requirements

### Requirement: A worktree's own submission of a shared number is the one it names

More than one branch can hold a number that a command gives with no
`--branch`. Then `sluice wait`, `status`, `amend` and `withdraw` SHALL
act on the submission that the worktree they run in made, as if
`--branch` named its branch. This SHALL hold only where that worktree made the
number on one of those branches only, and every branch could be read.
`sluice host` SHALL run a status, an amend and a withdrawal from the
folder that its client sent (record 0114 as amended 2026-11-26).

#### Scenario: Each command acts on the worktree's own submission

- **GIVEN** submission 26 queued on `bolt/a` from `unit/a`, and on
  `bolt/b` from `unit/b`
- **WHEN** `sluice status 26`, then `sluice amend 26 --priority 5` run
  in `unit/b`, and `sluice withdraw 26` runs in `unit/a`
- **THEN** the status shows `bolt/b`'s 26, `bolt/b`'s 26 has priority 5
  and stays queued, and `bolt/a`'s 26 is withdrawn with priority 0
  (`a_worktree_takes_its_own_submission_of_the_number` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A wait follows the worktree's own submission

- **GIVEN** submission 26 queued on `bolt/a` from `unit/a`, and on
  `bolt/b` from `unit/b`
- **WHEN** `sluice wait 26 --json` runs in `unit/b`
- **THEN** it follows `bolt/b`'s 26 to its landing, prints its record,
  and exits 0
  (`a_wait_in_the_worktree_follows_its_own_submission`)

#### Scenario: What the command gives names the worktree's branch

- **GIVEN** submission 26 queued on `bolt/a` from `unit/a`, and on
  `bolt/b` from `unit/b`
- **WHEN** `sluice withdraw 26 --as someone --json` runs in `unit/a`
- **THEN** it is refused as another's to withdraw, and its `next` is
  `sluice withdraw 26 --branch bolt/a --as agent-unit/a` and
  `sluice withdraw 26 --branch bolt/a --as operator`
  (`a_refusal_in_the_worktree_names_its_own_branch`)

#### Scenario: A worktree that made neither is refused

- **GIVEN** submission 26 queued on `bolt/a` and on `bolt/b`
- **WHEN** `sluice status 26`, `sluice amend 26 --priority 5`,
  `sluice withdraw 26` and `sluice wait 26` run in `unit/c`, which made
  neither, or in a folder that holds no session
- **THEN** each is refused naming `bolt/a` and `bolt/b`, as before, and
  neither branch changes
  (`a_worktree_that_submitted_neither_is_refused_naming_both`)

#### Scenario: A worktree that made the number on both is refused

- **GIVEN** submission 26 on `bolt/a` and on `bolt/b`, both from
  `unit/a`
- **WHEN** `sluice status 26` runs in `unit/a`
- **THEN** it is refused naming `bolt/a` and `bolt/b`
  (`a_worktree_that_submitted_it_on_both_is_refused_naming_both`)

#### Scenario: A branch that cannot be read keeps the refusal

- **GIVEN** submission 26 on `bolt/a` from `unit/a` and on `bolt/b` from
  `unit/b`, and `bolt/c`'s records refused with the words `<words>`
- **WHEN** `sluice status 26` runs in `unit/b`
- **THEN** it is refused naming `bolt/a` and `bolt/b`, then says
  `bolt/c could not be read: <words>`, and exits 2
  (`a_branch_that_cannot_be_read_keeps_the_refusal`)

#### Scenario: sluice host runs from the client's folder

- **WHEN** `sluice host` gets a status of a number, an amend or a
  withdrawal from a client in `/w/feat`
- **THEN** it runs the command with `/w/feat` as its worktree
  (`a_status_runs_as_from_its_clients_folder` in
  `crates/sluice/src/host_process/reads.rs`, and
  `an_amend_and_a_withdrawal_run_as_from_their_clients_folder` in
  `crates/sluice/src/host_process/changes.rs`)

### Requirement: A submit's answer gives the wait that names its branch

The answer of a submit that does not wait SHALL give the command that
follows its submission from any folder, `sluice wait <id> --branch
<branch>`. Under `--json`, it SHALL be the field `wait`, for each
submission of several worktrees too. In words, it SHALL be the line
`sluice wait <id> --branch <branch> follows it`, after the submission's
other lines (record 0114 as amended 2026-11-26).

#### Scenario: Under --json and in words

- **GIVEN** submission 26 queued on `bolt/a`
- **WHEN** `sluice submit bolt/b --no-wait --json` runs in `unit/b` and
  gets number 26, and `sluice submit bolt/a --no-wait` runs in `unit/c`
  and gets number 27
- **THEN** the first prints `{"id":26,"branch":"bolt/b","ahead":[],
  "wait":"sluice wait 26 --branch bolt/b"}`, the second prints
  `submission 27 to bolt/a · 1 queued ahead (#26)` and then
  `sluice wait 27 --branch bolt/a follows it`, and the first `wait`
  follows `bolt/b`'s 26 to its landing from any folder
  (`a_submits_answer_gives_the_wait_that_names_its_branch`)
