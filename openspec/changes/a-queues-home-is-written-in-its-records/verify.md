# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `b2ac1ea4`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools`, whose head
`48a9c3d5` it holds (merged in at `cfe6b529`), on 2026-11-15 with sluice
0.93.2 and moon 2.6.1.

## Outcome

**Not ready to archive.** The bolt's gate passes at this commit. So do
the tools-tier targets that hold the tasks' tools tests.

The work falls short of its own spec in one place. The queue-home spec
and design D2 still say that any two homes of a branch that are both open
or moving refuse the branch. The build refuses only one case: this host's
own home is open while a live remote home came from this host's own
files. Record 0122's amendment and rule core.59 say the same as the build.
Archiving now would write into `openspec/specs/queue-home/spec.md` a
requirement the code does not keep. That requirement also contradicts two
other scenarios of the same spec. Task 3.1's text describes a lookup order
the build does not use, and its record calls that order the path not
taken.

Every one of the 28 tasks is ticked. Each test a task names exists where
the task says. Every scenario of the four delta specs maps to a test, and
`openspec validate --strict` finds the change valid.

## Whole checks

I ran `sluice exec bolt/sluice-is-refactored-and-runs-on-pools` once at
`b2ac1ea4`. It runs the bolt's gate, `#check-fast` (S66), over the checks
`b2ac1ea` affects against the bolt's `48a9c3d`. It exited 0 with 12 tasks
passed and none failed:

| Task | Outcome |
|---|---|
| site:prose, site:rules, nix:typecheck | passed |
| root:adrs, root:changes, root:format (`6ea15f59`), root:lint | passed |
| core:test (164), fakes:test (17), seams:test (5) | passed |
| conformance:test (every target, `homes`, `peers` and `reading` among them) | passed |
| cli:test (`10d4525f`) | passed |

`cli:test` ran every target of `cargo test -p sluice -- --skip sandbox`,
with no failures. This time `host_process` passed 39 of 39: the bolt's
#86, merged in at `cfe6b529`, fixed the failure the last verify found
there. `cli` passed 223 tests and `local_files` passed with task 1.3's
test.

sluice recorded none of the 12 results. I wrote this report into the
worktree while the run was going, so at its end the worktree held
`verify.md` beyond `b2ac1ea`'s head (S66). This affects only whether the
results were recorded, not the outcome. moon also warned on each task
that it could not package outputs into `.moon/cache/outputs/` under the
place.

The tools tier is main's gate, not the bolt's, so `#check-fast` builds
no test with `--features tools`. Tasks 1.4 to 1.6, 2.4, 2.5, 3.2, 4.4,
4.6 and 4.7 name checks in that tier. I ran their four targets whole,
once, through sluice against the bolt: `sluice exec
bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice
--features tools --test remote_refs --test remote_branch --test peers
--test pull_requests`. It exited 0:

| Target | Passed | Failed |
|---|---|---|
| peers | 13 | 0 |
| pull_requests | 157 | 0 |
| remote_branch | 34 | 0 |
| remote_refs | 37 | 0 |

Every test those tasks name is in these targets.

## Findings

### 1. The queue-home spec refuses two homes the build does not refuse

`specs/queue-home/spec.md`, requirement "Every process reads the home
from the records":

> Two homes of one branch that are both open or moving SHALL refuse that
> branch alone, naming both (26).

Its scenario "Two open homes" has the same WHEN: this host's own files and
origin's state of `bolt/x` both hold an open `home.json`. Design D2 says
the same: "Two homes both `open` or `moving` refuse the branch alone,
naming both (26)."

The build, in `crates/sluice/src/profiles/records.rs` `find_route`
(lines 346 to 364), refuses a branch only where this host's own home is
`open` and a live remote home's `came_from` names this host's own files
and this host. It routes the following cases to this host's own files
with no refusal:

- this host's own home is `open` and origin's home is open, having come
  from another host or been opened by a policy;
- this host's own home is `moving` and origin's home is open.

Record 0122's amendment of 2026-11-14 chooses this on purpose: "One queue
in two homes is told apart from a queue not yet moved ... *Not taken:*
refusing any two open homes, which would refuse every host whose own
queue had not yet moved, so none ever could." Rule core.59, added by
this change, also leaves it open: "Not ruled out: ... a host's own open
home beside a remote home another host's queue or a policy opened, which
its round moves (design D5)". The test
`two_open_homes_refuse_the_branch_naming_both` sets origin's `came_from`
to this host (`host-a`). So it shows only the narrow case.

The build's reading is the one the spec needs. Read as written, the
requirement contradicts the spec's own scenarios:

- "Two hosts each held #52": once A's queue is open on origin, B's own
  home of `bolt/x` is still open, so B would be refused and never carry.
- "Stopped after the new home was written": this host's own home is
  `moving` while origin's is open, so the next round would be refused
  and never close the move.

The statement asks for no refusal of this kind. C.6 at agentplot/blueprints
`e966dcb` says "A branch's queue has one home" and nothing about refusing
two. The broad wording is the change's own, in the delta spec and in D2.

CLAUDE.md requires that "where the work changes a requirement, it
carries that change in the same branch". The work narrowed this
requirement and did not carry the change in its delta spec. The fix
belongs in this change, before it is archived:

- amend the requirement and the "Two open homes" scenario in
  `specs/queue-home/spec.md` to say what is refused: this host's own home
  open while a live remote home came from this host's own files;
- say in the requirement, or in a scenario, that the round moves a
  host's own open home beside a remote home that another host's queue or
  a policy opened;
- make the same change in design D2.

### 2. Task 3.1 describes the lookup order the build rejected

Task 3.1 says a status by number and each act by number "read the open
home first and, for a number it does not hold live or finished, each
closed home this host reached it through". The build does the reverse.
`Sluice::reach` in `crates/sluice-core/src/lib.rs` reads the pointer of
each closed home this host reached the branch through first, then the
open home, then the closed homes' own records. Record 0122's amendment
names the task's order as the path not taken: "*Not taken:* the open home
first, which would answer another host's submission for a number given
up". The build's order is the one the scenario "Two hosts each held #52"
needs, and the task's named tests pass on it. The task's text should say
what was built.

## Tasks

All 28 tasks in `tasks.md` are ticked, and each test a task names exists
where the task says:

- 1.1: `sluice-core/src/records.rs`. 1.2: `sluice-fakes/src/lib.rs`.
  1.3: `tests/local_files.rs`.
- 1.4, 1.5, 2.4, 2.5: `tests/remote_refs.rs`. 1.6, 4.6:
  `tests/remote_branch.rs`.
- 2.1 to 2.3, 2.5's second half, 3.1: `sluice-conformance/tests/homes.rs`.
- 3.2, 4.7's first test: `tests/peers.rs`. 3.3, 5.5: `tests/cli.rs`.
- 4.1, 4.3: `sluice-conformance/tests/peers.rs`.
- 4.4, 4.7's second test: `tests/profiles/pull_requests.rs`.
- 4.5: `sluice/src/instance.rs` and `sluice-core/src/policy.rs`. 5.1:
  `sluice-conformance/tests/reading.rs`.

The tests that left with 1.4 and 2.6,
`the_first_hosts_queue_moves_and_the_others_is_left_behind` and
`a_submission_left_behind_is_submitted_again_naming_its_old_id`, are gone.
No `migrate`, `left_behind`, `forget_left_behind`, `adopt_left_behind`,
`leave_behind` or `sluice-remote/moved/` is left in the crates or the
site. The `left_behind` in `tests/profiles/places.rs` belongs to a place
and has nothing to do with this change. The `Moved` still in
`sluice-seams` is the branch seam's result for a move of a ref, not the
records' old migration type.

Every test a task names passed in this stage: in the bolt's gate, or in
the tools-tier run above. Tasks 2.6, 5.2, 5.3 and 5.4 hold by
`root:lint`, `site:prose`, `root:adrs` and `site:rules`, each of which
passed.

Record 0122 has its build amendment and record 0116's routing half is
marked as replaced (5.3). Rules core.59 and core.60 are in
`docs/architecture/core.md`, and core.27's details point to core.59
(5.4). `site/src/content/docs/reference/state.md` and `commands.md` name
the home, `carried_from`, `kept` and the three log lines (5.2).

Task 5.6's `moon.yml` change reached the bolt through #86 (`d2472262`)
and came into this branch with the merge at `cfe6b529`. This branch's
`moon.yml` is the bolt's.

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict` finds
the change valid. The MODIFIED and REMOVED requirements of `peer-lead` and
`remote-records` name requirements that `openspec/specs/` holds, and
`queue-home` is a new capability. Each scenario maps to a test:

| Spec: scenario | Test |
|---|---|
| queue-home: The first submission writes the home; A branch with no signers | `the_first_write_writes_the_home_its_policy_chose` (both cases) |
| queue-home: A policy that names another home does not move the reading | `a_branchs_home_is_read_from_its_records_not_its_policy` |
| queue-home: A host with no records of the branch | `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote` |
| queue-home: Two open homes | `two_open_homes_refuse_the_branch_naming_both`, only where origin's home came from this host (finding 1) |
| queue-home: A move waits for a combination under test | `a_policy_naming_another_home_writes_a_move_and_opens_no_combination`, `a_move_waits_for_the_combination_under_test` |
| queue-home: The policy names the open home again | `a_policy_naming_the_open_home_again_ends_the_move` |
| queue-home: A host's queue moves to origin | `a_move_carries_every_live_submission_with_its_number_and_state`, `a_hosts_queue_moves_to_its_remote_with_its_numbers` |
| queue-home: A submit racing the carry | `a_submit_racing_the_carry_is_written_in_the_new_home`, `a_submission_made_while_moving_is_carried` |
| queue-home: Stopped after the new home was written | `a_move_stopped_after_the_new_home_was_written_is_closed_by_the_next_round`, `a_carry_stopped_after_the_remote_was_written_is_closed_by_the_next` |
| queue-home: Two hosts' submissions come home | `a_branch_comes_home_each_host_taking_its_own_submissions`, `a_round_carries_its_hosts_submissions_home_before_it_reads_the_branch` |
| queue-home: A host that has not come back | `a_submission_left_for_its_host_says_it_waits_for_it` |
| queue-home: Numbers past the remote's count | `a_hosts_queue_moves_to_its_remote_with_its_numbers` |
| queue-home: Two hosts each held #52 | `a_number_the_remote_holds_is_carried_under_the_next`, `a_status_of_a_carried_number_names_the_number_it_has_now` |
| queue-home: A submission landed before the move | `a_submission_finished_before_the_move_is_read_in_the_closed_home` |
| queue-home: A host's settings name a lead; A policy names how a branch runs | `no_host_setting_chooses_how_a_queue_runs`, `no_policy_key_chooses_how_a_queue_runs` |
| queue-home: A move waiting | `the_overview_names_each_branchs_home_and_its_move`, and 5.5's status tests |
| submission-wait: The queue moves while a wait follows; Carried under another number | `a_wait_follows_its_submission_to_its_queues_new_home` |
| peer-lead: The former lead stops after a takeover; A lead that went quiet; Clocks that disagree | `a_quiet_lead_is_taken_after_the_lease_by_the_takers_own_observation`, `a_lead_that_writes_keeps_the_branch` (conformance `peers`) |
| peer-lead: A lead the branch no longer accepts | `a_lead_the_branch_no_longer_accepts_is_taken_after_the_lease` |
| peer-lead: A session host submits to a bolt held on origin | `a_host_whose_signature_the_branch_does_not_accept_never_leads` |
| peer-lead: A host with an unlisted key | `a_host_with_an_unlisted_key_never_leads`, `a_host_the_branch_does_not_accept_takes_no_request_in` |
| peer-lead: A bolt whose signers went | `any_host_leads_a_remote_home_whose_policy_names_no_signer` |
| peer-lead: A run longer than the lease | `a_lead_keeps_its_branch_through_a_run_longer_than_the_lease` |
| peer-lead: Nothing live | `a_lead_with_no_live_run_writes_nothing_to_keep_it` |
| remote-records: Signers move the state to the remote; No signers, no change | `the_first_write_writes_the_home_its_policy_chose` |

I read the move tests in `homes.rs` against their scenarios. Each one
asserts what its scenario's THEN says.
