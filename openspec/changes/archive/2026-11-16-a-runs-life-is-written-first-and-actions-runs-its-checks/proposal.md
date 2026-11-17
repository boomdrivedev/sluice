## Why

Record 0122, agentplot-design's, accepted 2026-11-13, has two halves.
The change `a-queues-home-is-written-in-its-records` built the first, a
queue's home. This change builds the second: a run's life written in its
branch's home before anything acts on it. It also builds record 0124's
landing the forge refuses, and the GitHub Actions pool of record 0119,
whose change `actions-pool` on sluice's main is its proposal, built and
archived here (statement in boomdrivedev/blueprints,
`design/sluice/requirements.md`: 24, 60, 66, 72 to 79, 87, 88, 107,
108, 111, 113, 114, 115, 152, C.2, C.6, C.8; rules core.5, core.7,
core.16, core.22).

What sluice's code holds at e56a904b is what the records replace:

- **A run's state has no one owner.** `RunState`
  (`crates/sluice-core/src/records.rs`) is open, starting, testing,
  passed, failed or timed out, and a run goes back to open whenever any
  step finds a reason: `poll` reopens a claim it reads as lapsed, a run
  whose executor was lost, and a run that left checks missing;
  `finish_run` reopens a whole run with `no_replay` for one unproven
  replay. A reopened run loses every outcome it had.
- **Acts come before the writes that name them.** `start_run` makes the
  checkout and starts the checks and only then writes `Testing`, so a
  host that stops between leaves a `Starting` run whose checks run,
  which the next round releases and opens again. `launch_on_pools`
  dispatches a pool's job and only then writes its claim.
- **A place git left locked cannot be released.** A `git worktree add`
  that never finished leaves the worktree locked `initializing`, and
  every release after it fails on `cannot remove a locked working tree`
  (wldn's gate box, switchboard-kit's
  `sluice/try/bolt/productions-first-install-stands-as-you-decided/6`,
  2026-11-13, signal `2026-11-13-agentplot-operator-4dd5f9aa/01`).
- **A change of lead changes runs.** A lead does not show its sign of
  life, so the next lead reads the old lead's own claims as lapsed and
  their green runs as timed out (switchboard-kit's floci bolt,
  submissions 97 and 98, combination d85ada7, signal
  `2026-11-13-swb-3-ops-52a711cd/01`).
- **The limit counts from the combination's opening.** `poll` measures a
  candidate's time from `opened`, so a run waiting for room reads as
  testing past its limit, and a run no host ever claimed is written
  timed out with every required check failed and blamed on the tip
  (#99 and #100, signal `2026-11-13-agentplot-planner-74fcda5d/01`).
- **A report is read before its run has ended.** A lead wrote
  combination 9a5118a failed, listing 38 checks with no recorded run,
  from a report of try/23 that held one pass while try/23 still ran, and
  blamed #103 (signal `2026-11-13-agentplot-ops-8a4ae6be/01`).
- **A landing the forge refuses is pushed again every round.**
  `PushedRefs::leased` reads a refusal at the ref only from `[rejected]`,
  so git's `[remote rejected]` is an error, `perform_landing` keeps the
  intent and pushes again each round, and `withdraw_in` refuses the
  landing entry, so nothing can take it out (switchboard-kit #2021, signal
  `2026-11-13-agentplot-ops-b29a54cd/01`).
- **The pool cannot report.** `launch`, `job_alive` and `wake` reach
  GitHub, but no workflow exists, no command a job runs, and a `github`
  signer's records count for nothing, since nothing verifies a keyless
  signature (`SignerKey::Keyless`).

## What Changes

- **A run's states are written before any act.** A run is `open`,
  `claimed`, `running`, `ended` or `closed`, in its branch's home, written
  only by the instance whose claim holds it or the lead that opened it.
  Making its checkout, starting its checks, dispatching a pool's job,
  recording its results, releasing its checkout and landing each happen
  only after the state naming them is written, and each leaves a proof
  the next reader checks, so an act found done is not done again and one
  found half done is finished or undone.
- **A checkout git left locked mid-way is undone.** A checkout sluice
  made that git lists locked `initializing` is unlocked, removed and
  pruned before it is made again or released; any other lock holds it,
  named with git's words.
- **The limit is the run's.** It counts from the run's own `running`. An
  open run reads as waiting for room since it opened. One no live
  executor may take ends at the combination's limit with no outcome:
  the combination is not passed, no check is written failed, nobody is
  blamed, and each check says who turned it down and why.
- **A change of lead changes no run.** Every instance holding a claim
  shows its sign of life, the lead among them; a claim lapses only when
  its own holder's does; the holder writes its own report whichever
  instance leads; and a run's written end is never rewritten.
- **A report comes only once its run has ended**, and holds an outcome
  for each check the run was given. A check it holds none for has no
  outcome: never failed, nobody blamed.
- **A follow-up run holds only what a run left undecided.** A run that
  ends with unproven replays or checks with no outcome keeps every other
  check's outcome, and the lead opens a follow-up run of only those
  checks, the unproven ones with no replay. The combination's verdict is
  read over a run and its follow-ups.
- **A landing the forge refuses is held.** With the branch read where the
  landing expected it, the refusal is written with the forge's words,
  logged once, shown on the branch and each of its submissions, and
  never pushed again by itself. Its submissions may be withdrawn, a
  submission from the same place withdraws its own, and once one leaves
  the rest form again. A wait ends with exit 6 and the forge's words.
  `sluice push <branch>` pushes it again on the operator's word.
- **GitHub Actions runs the checks no standing host takes.** `sluice init
  actions` writes `.github/workflows/sluice.yml` and `sluice-pr.yml`, and
  adds `[signers.forge]` and `[pools.forge]`. A run on the pool is one
  workflow run of two jobs, through the same states: a run job on the
  run's system's runner that holds no key and no identity token, and a
  sign job that runs none of the combination's code, signs only the keys
  the tree bears out with the job's keyless identity, and records the
  run's end under its claim. `sluice job run`, `sluice job sign` and
  `sluice job record` are what the jobs call.
- **Keyless signatures verify on every host from the records alone**,
  with `sigstore-verify` against the public-good and GitHub trusted roots
  that sluice carries, calling no service. A certificate counts only when
  its workflow, ref, repository and issuer match `[signers.<name>]
  github`.
- **A pull request's head run counts.** A run of `sluice-pr.yml` on a
  head, signed by `sluice.yml`'s sign job on the signer's branch, is
  recorded under its keys and read by a combination that shares them
  (113, S40). `pull_requests.checks` naming sluice's own check is refused.
- **`sluice forge check` covers the pool**: the workflow, the permission
  to dispatch it, the environment's branch rule and the forge signer.

## Capabilities

### New Capabilities

- `run-life`: a run's five states, written before any act; each act's
  proof and what the next reader does with an act found done or half
  done; the limit counted from `running`; a run no live executor may take;
  a report only once its run has ended; follow-up runs; a run's written
  end standing (record 0122; 60, 66, 75, 77, 87, 88, 108, 114).
- `refused-landing`: a landing the forge refuses held with its words,
  its submissions their submitters', the wait's exit, and the operator's
  push (record 0124; 5, 24, 67, 122, 152; core.5).
- `actions-pool`: GitHub Actions as a pool: the workflows `sluice init
  actions` writes, one workflow run per run on its system's runner
  through the run's states, run and sign jobs apart, the app's key only
  in the signer's branch's sign job, and `sluice job` (records 0119,
  0122; 88, 107, 111, C.2 launched, C.8).

### Modified Capabilities

- `run-claims`: a claim lapses only by its own holder's sign of life,
  which every claim holder shows, the lead among them, and the holder's
  report is taken whichever instance leads (73, 77).
- `run-outcome`: a check the limit ended is a running run's alone, its
  limit counted from `running`; a combination whose checks could not run
  is tested again by a follow-up run.
- `run-progress`: a combination's line gives each run's state, an open
  run waiting since it opened and a running run's time of its own limit.
- `pass-proof`: a combination whose passes had no proof is tested again
  by a follow-up run of those checks.
- `run-signing`: a `github` signer's records are verified keylessly and
  count; a replay no counted pass proved runs again in a follow-up run.
- `place-release`: a checkout git left locked `initializing` is undone.
- `forge-identity`: `sluice forge check` covers the pool.
- `pull-request-intake`: a run on a request's head counts toward its
  combination; `checks` naming sluice's own is refused; the refusal of a
  pull request table on records each host keeps names `sluice init
  actions`.
- `submission-wait`: exit 6 for a submission whose landing the forge
  refused.
- `check-systems`: a run no live host or pool can take ends with no
  outcome at the limit, blaming no one.
- `host-requirements`: a run no host takes for its engine ends the same
  way.

## Impact

- **Code.** `crates/sluice-core`: `records.rs` (`RunState`, the run's
  claim, `running`, `ended` and `closed`, follow-ups, the candidate's
  `not-passed`, the landing's refusal, events), `step.rs` (`decide`,
  `perform`, `poll`, `finish_run`, `start_run`, `launch_on_pools`,
  `execute`, `perform_landing`, `judge`), `ops.rs` (`withdraw_in`,
  status), `overview.rs`, `verdict.rs`. `crates/sluice-seams` (`Moved`'s
  forge refusal, a run's start proof, a pool job's start).
  `crates/sluice-fakes` (a forge that refuses a push, a run started with
  no write after it, a checkout left locked). `crates/sluice`: profiles
  `places.rs`, `pushed_refs.rs`, `checks.rs`, `remote_refs.rs`,
  `records.rs`, `github/{actions,check,pulls}.rs`, a keyless verifier and
  workflow templates under `profiles/github/`; commands `sluice job`,
  `sluice init actions`, `sluice push`; the wait's exit 6.
- **Dependencies.** `sigstore-verify` and `sigstore-trust-root`
  (sigstore/sigstore-rust), pinned.
- **Tests.** A new conformance target
  `crates/sluice-conformance/tests/run_life.rs`; `peers.rs`,
  `landing.rs`, `could_not_run.rs`, `unkept_proof.rs`, `scenarios.rs`
  there; `crates/sluice/tests/{places,pushed_refs,peers,cli}.rs`,
  `tests/profiles/forge.rs` and `tests/profiles/pull_requests.rs`;
  recorded bundles as fixtures.
- **Records.** A run's `state` takes the five words and gains `opened`,
  `claim`, `running`, `ended`, `closed` and `follows`; a candidate may
  read `not-passed`; a submission may read `refused`; a landing gains
  `refused`; `sluice/log@1` gains `opened`, `running`, `ended`, `closed`
  and `forge_refused`. A
  host's queues are drained before the release that brings this, so no
  record of the older shape is read (principle 12).
- **Gate.** actionlint covers the workflow templates; the recorded
  bundles are excluded from formatting by name.
- **Docs.** The site's state, commands and configuration references, the
  executors and systems guide, a new GitHub Actions guide, the pull
  requests guide, the roadmap, the setting-up skill and the README; record
  0122 amended with the build's choices; rules core.61 and core.62.
- **Family.** flume reads a submission's `refused` state, and crew and
  `wt submit` a wait's exit 6 (design, Looking across the family).

## Out of scope

- **A GitHub Action running a queue whole**, the round job that leads a
  branch with every host off, a wake after a finished run and the
  schedule's backstop: `a-github-action-runs-a-queue-whole`, after this
  change. Until it lands, a branch on the pool lands only while a host
  that leads it is up; actions-pool's round job goes there.
- **The CodeBuild pool** of record 0119: a change of its own, built on
  this one's states.
- **A run's signed record of in-toto's Test Result kind** (record 0077;
  core.40): the sign job signs the statement sluice signs today.
- **Signing the landing verdict, a pool on another forge, `sluice exec
  --remote`**, as in `actions-pool`.
