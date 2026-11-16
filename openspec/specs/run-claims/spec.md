# run-claims Specification

## Purpose
Decides which host runs which combination's checks when many hosts
serve one branch: a host takes a run by recording a claim, earliest
first, up to its own bound, and a claim lapses when its host stops
(75 to 77, 81, 107).

## Requirements

### Requirement: A run is taken only by a recorded claim
The lead SHALL open combinations within the branch's `parallel` window
and assign them to no host. A host SHALL run a combination's checks only
after its claim on that run is written to the branch's state, and only
while the host's bound has room, counting every check running on the
host, whatever repository it serves (71, 75).

#### Scenario: Two hosts reach for one run
- **WHEN** hosts A and B both try to claim the same run from the same version of `main`'s state
- **THEN** exactly one claim is written, and only that host runs the checks

#### Scenario: A full host claims nothing
- **WHEN** host A's bound is full
- **THEN** host A claims no run, whatever the window allows

#### Scenario: A host full with another repository's runs claims nothing
- **WHEN** host A's bound is 2 and two runs of another repository are running on it
- **THEN** host A claims no run of `main`, whatever the window allows

### Requirement: Runs are claimed earliest first
A host SHALL NOT claim a later open combination of a chain while an
earlier open combination of that chain is unclaimed (76).

#### Scenario: The first combination first
- **WHEN** combinations #1 and #1+#2 of `main` are open and neither is claimed
- **THEN** a host with room for one run claims #1

### Requirement: Each host shows it is alive and how much room it has
Each host running sluice for a repository SHALL rewrite
`refs/sluice/hosts/<host>` on its remote at the cadence of its
sign-of-life bound, with the checks running on the host, every
repository's, and the room the host's bound has left. Only that host
SHALL write its ref.

#### Scenario: A host's sign of life
- **WHEN** `sluice run` is running on host A with a bound of 4 and one run under test
- **THEN** `refs/sluice/hosts/A` is rewritten at least once per sign-of-life bound and says one run held and room for three

#### Scenario: Another repository's runs leave less room
- **WHEN** host A's bound is 4, one run of `main` is under test, and one run of another repository on host A is running
- **THEN** `refs/sluice/hosts/A` says two runs held and room for two

### Requirement: A claim lapses with its host's sign of life
A claim SHALL lapse once a reader has seen its host's sign of life
unchanged for the sign-of-life bound, never for the length of its run.
A lapsed claim's run SHALL have no outcome, and a result reported under
a claim its host no longer holds SHALL NOT be written (60, 77).

#### Scenario: A host stops mid-run
- **WHEN** host A claimed a run of `main` and is switched off
- **THEN** after the sign-of-life bound the claim lapses, the run is open again, and another host claims it

#### Scenario: A long run keeps its claim
- **WHEN** a run takes 40 minutes and its host keeps rewriting its sign of life
- **THEN** the claim holds until the run reports, within the branch's `timeout`

#### Scenario: A late report
- **WHEN** host A returns after its claim lapsed and tries to report the run's result
- **THEN** the result is not written to the branch's state, and the run's outcome is the one its new claim reports

### Requirement: A returning host releases its checkouts before it claims
A host SHALL claim nothing while a checkout it made belongs to a
combination that has a result or is obsolete, or to a claim it no longer
holds; it SHALL release those first (25, 34, 81).

#### Scenario: Back after a lapse
- **WHEN** host A returns with a checkout left from a lapsed claim, and the run has since passed on host B
- **THEN** host A releases that checkout, running its release command, before it claims any run

### Requirement: Runs go to standing hosts before pools
The lead SHALL start a run on a pool only when no live host able to take
it has room by its last sign of life: one of the run's system, whose
signer the branch trusts. Among pools, the order the policy lists them
SHALL decide (107).

#### Scenario: The box has room
- **WHEN** a run needs `linux-x64`, and the `box` host signs for `linux-x64`, is trusted by `main`, and has room
- **THEN** the box claims the run and no job starts on the pool (S41)

#### Scenario: The box is full
- **WHEN** the same run is open and the box has no room
- **THEN** the lead starts the run on the first pool the policy lists that runs `linux-x64`

### Requirement: The policy lists pools in order
The policy SHALL accept `[pools.<name>]` tables, each with a `github`
workflow path, the `signer` its jobs sign as, and `runners`, the runner
label for each system it runs. A pool whose `signer` is not a `github`
signer, or whose runners name a system its signer may not claim, SHALL
be refused. The order written SHALL be the order pools are tried (107).

#### Scenario: A runner for a system the signer may not claim
- **WHEN** `[pools.forge] signer = "forge"` has a runner for `macos-arm64` and `[signers.forge] systems` lacks it
- **THEN** the policy is refused, naming `pools.forge` and `macos-arm64`

### Requirement: A claimed run's checkout is made at its combination

A host SHALL make a claimed run's checkout at its combination however
the claim reached it: a round's own start, a claim it read after its
round fetched the branch's combinations, or a pool's job. Where the host
lacks the combination, it SHALL first fetch the combination's own ref,
`refs/sluice/combinations/<branch>/<id>`, from the branch's remote, and
no other; a host that holds it SHALL fetch nothing for it (82, C.6;
record 0118).

#### Scenario: A pool's job on a machine that lacks the combination

- **WHEN** the lead launches a run of `bolt/x` on the forge's pool, and
  the pool's machine starts the claimed run holding none of its
  combination
- **THEN** the machine fetches the combination by its own ref, makes the
  checkout at it, and starts its tests
  (`a_pools_job_makes_its_place_at_a_combination_only_the_remote_holds`
  in `crates/sluice-conformance/tests/could_not_start.rs`)

#### Scenario: A combination only the remote holds

- **WHEN** host B opened combinations `c7` and `c8` of `main`, and host
  A, holding neither, fetches `c7`
- **THEN** host A holds `c7` alone, and a checkout made at it holds it
  (`a_combination_only_the_remote_holds_is_fetched_and_a_place_made_at_it`
  in `crates/sluice/tests/remote_refs.rs`)

#### Scenario: A host that holds the combination

- **WHEN** host `a` claims a run of a combination its round fetched
- **THEN** it starts the run's tests with no fetch of the combination
  of its own
  (`a_host_that_holds_the_combination_fetches_nothing_more` in
  `crates/sluice-conformance/tests/could_not_start.rs`)
