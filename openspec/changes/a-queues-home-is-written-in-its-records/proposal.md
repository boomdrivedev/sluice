## Why

Record 0122, agentplot-design's, accepted 2026-11-13, writes a queue's
home in its branch's records: every process reads it there, a move
between the host's own files and the remote's refs is written step by
step, and a branch held on its remote has one lead at a time, which
never moves under a live run. Record 0093 as amended on the user's card
of 2026-11-08 ("Yes: machines choose how sluice runs, the repository
says what must pass") adds that a machine whose signature a branch does
not accept leads none of its rounds and runs none of its checks, and
that no setting names how a queue runs. The statement carries that half
in 58, 72, 135, 137 and C.8 (boomdrivedev/blueprints,
`design/sluice/requirements.md`). Record 0122 has C.6 amended to
match; that amendment is not on blueprints' main at 786d040, and this
change cites C.6 as the record amends it.

What sluice's code holds at d6d11623 is what the record replaces:

- `Records::route` (`crates/sluice/src/profiles/records.rs`) works out
  from the policy it reads whether a branch's records are the host's
  files or a remote's refs, and keeps the answer for the process's life.
  Two processes that answer differently each hold a lock and number
  entries of their own; on 2026-11-12 switchboard-kit's main gave #52
  and #53 twice and bolts moved between the host and GitHub (records
  0114, 0116).
- A move happens as a side effect of whichever lock first routes the
  branch to its remote (`migrate`, design D10 of ADR 0037's build): the
  first host's queue is pushed whole, every other host's queue is left
  behind and submitted again under new numbers, and nothing moves a
  branch back when its signers go.
- `Sluice::keeps` (`crates/sluice-core/src/step.rs`) gives the lead to
  any instance once the records' version has stood a lease, with no
  look at whether the branch accepts its signature, and a lead writes
  only when its round changes something. A run longer than the lease
  writes nothing, so another host takes the lead under it. On
  2026-11-13 chuck-herdr-alpha and chuck-sluice-alpha took the lead of
  switchboard-kit's `bolt/a-function-that-calls-aws-is-proved-on-floci`
  from each other every 6 to 14 minutes, and each new lead read the
  running claim as lapsed and wrote a run that finished 165 tasks with
  none failed as timed out (signals
  `2026-11-13-agentplot-ops-f42e2e42/01` and
  `2026-11-13-swb-3-ops-52a711cd/01`).

## What Changes

- **A queue's home is a document of its records.** Each home of a
  branch holds `home.json`: where the queue lives, the revision of the
  policy that chose it, and whether it is open, moving or closed with a
  pointer to the home that took it.
- **Every process reads the home from the records.** The records seam's
  binding routes a branch by this host's own `home.json`, then by a
  remote's, as the host process's tick last fetched it, and reads it
  again at every pass and lock. Only a branch with no home anywhere is
  routed by its policy, and its first write writes the home that policy
  chose.
- **A move is written step by step.** A round that reads a policy at the
  branch's head naming another home writes `moving` in the current home
  and opens no combination. Once no combination is under test, it
  carries every live submission to the new home with its number and its
  state, then closes the old home with a pointer. A round that stops
  between writes leaves what the next round finishes.
- **Numbers survive a move.** A carried submission keeps its number, and
  the new home's count is raised past it. A number the new home's branch
  already holds is carried under the next one, which the old home's
  pointer and the submission's record name.
- **A branch comes home to each host's own files.** When its policy
  stops naming signers, the lead closes the remote home once no
  combination is under test, and each host carries the submissions it
  made into its own files at its next lock of the branch.
- **A wait follows a submission to where it lives**, through a closed
  home's pointer, and a status of a carried number names the number it
  has now.
- **Only a host the branch accepts leads it.** An instance whose
  signature the policy at the branch's revision does not accept never
  takes or keeps the lead of a branch held on its remote, takes none of
  its pull requests in and claims none of its runs; it submits and
  leaves the queue to one the branch accepts. A remote home whose
  branch's policy names no signer may be led by any instance, to carry
  its queue home.
- **The lead never moves under a live run.** While any run of the branch
  is claimed or under test, the lead writes the branch's records at
  least once a lease, every half lease by its own clock, so no other
  instance's lease passes.
- **No setting names how a queue runs**, in a host's settings or a
  branch's policy: tests pin it.
- **The home shows.** A branch's status and the overview name its home
  and a move in progress.
- **The left-behind design goes**: `migrate`, the host's
  `sluice-remote/moved/` notes, `State::left_behind`,
  `State::forget_left_behind` and the round's `adopt_left_behind`.

## Capabilities

### New Capabilities

- `queue-home`: a branch's queue has one home written in its records,
  read from there by every process; a move between homes, step by step;
  numbers across a move; a branch coming home; what a status shows; and
  no setting naming how a queue runs (record 0122; 135, 137, C.6, C.8).

### Modified Capabilities

- `remote-records`: a policy with signers chooses the remote home for a
  queue that has none; the left-behind move goes.
- `peer-lead`: only a host the branch accepts leads it, and the lead
  writes at least once a lease while a run is live (58, 72, 137).
- `submission-wait`: a wait follows its submission to its queue's new
  home.

## Impact

- **Code.** `crates/sluice-core/src/records.rs` (`HomeDoc`, the
  submission's `carried_from`, the branch's `kept`, events `moving`,
  `stayed`, `carried`), `step.rs` (`keeps`, `decide`, the move),
  `ops.rs` (`moved_to`, status), `overview.rs`; `crates/sluice-seams`
  (`State::carry`, `Carried`; `left_behind` and `forget_left_behind`
  go); `crates/sluice-fakes` (a branch's two homes);
  `crates/sluice/src/profiles/{records,local_files,remote_refs}.rs`;
  `crates/sluice/src/lib.rs` (wait, status).
- **Tests.** A new conformance target
  `crates/sluice-conformance/tests/homes.rs`; `peers.rs`, `engine.rs`,
  `places.rs` and `could_not_start.rs` there open runs with a lead the
  branch accepts; `crates/sluice/tests/remote_refs.rs`, `local_files.rs`,
  `peers.rs` and `pull_requests.rs`.
- **Records.** `home.json` in every home; a submission's optional
  `carried_from`; the branch document's optional `kept`; `sluice/log@1`
  lines `moving`, `stayed` and `carried`. The `sluice-remote/moved/`
  notes are read by nothing.
- **Docs.** The site's state and commands pages; record 0122 amended by
  the build's choices; rules core.59 and core.60, and core.27's details.
- **Hosts.** A host's queues are drained before the release that brings
  this, so no record of the older shape is read (principle 12).

## Out of scope

- **A run's life written before it acts**: its states, the time limit
  counted from its own start, a claim that lapses only by its own lease
  whichever instance leads, the lead showing its sign of life as any
  claim holder does, and an unproven replay rerun alone. That is
  `a-runs-life-is-written-first-and-actions-runs-its-checks` (record
  0122's second half; record 0119). This change stops the lead moving
  under a live run; that one makes a lead that does move change no run.
- **`sluice daemon`**: `sluice-runs-as-a-daemon`.
- **A GitHub Action running a queue whole**:
  `a-github-action-runs-a-queue-whole`.
- **Which remote a branch's revision is read from** (rule core.27), the
  branch seam's, unchanged: the home is the records seam's.
