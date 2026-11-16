# Design

## Context

Record 0122's second half, a run's life, with its amendments of
2026-11-13 on main (sluice #148, a change of lead keeps a claim; #149,
an untaken run blames no one; #151, a report comes at a run's end);
record 0124, a landing the forge refuses; and record 0119's GitHub
Actions pool, whose change `actions-pool` is this change's proposal for
the pool. The statement holds as written: 66 bounds a candidate's checks
and does not say from when; 87 reads a run no executor takes as not
passed at the limit; 60, 77 and 114 give a lapsed claim, a lost run and
a check the report holds nothing for no outcome; 108 runs an unproven
replay again; 152 is record 0124 whole; C.2's launched row and C.6's
keyless signature are the pool. Nothing here changes a clause.

What the code does at e56a904b:

- **States.** `RunState` (`crates/sluice-core/src/records.rs`) is
  `Open`, `Starting`, `Testing`, `Passed`, `Failed`, `TimedOut`, written
  only by `step.rs`. A run goes back to `Open` from nine places
  (`recover`, `unmade`, `reopen`, `poll`'s lapse and its lost run,
  `finish_run`'s unproven and uncounted readings, `read_not_run`,
  `execute`'s lost run), each clearing what the run had; `recover`
  leaves `host` set, so a signed branch's reopened run is claimed by
  no one.
- **Write and act.** `decide` writes and `perform` acts, but `start_run`
  makes the checkout, starts the checks and only then writes `Testing`;
  a host that stops between leaves `Starting`, and `perform_starts`'
  stopped-round branch or `recover` releases the checkout under the
  running checks and opens the run again. `launch_on_pools` dispatches
  the job, then writes the claim. Releases are both orders.
- **Leads and claims.** Only `execute`, a host that does not lead,
  writes `refs/sluice/hosts/<host>`; a lead never does, so the next lead
  observes a sign of life that never moves and lapses the old lead's
  claims after `[peers] alive`.
- **The limit.** `poll` reads it from the candidate's `opened` and, at
  the limit, writes every open run the lead itself may not take
  `TimedOut` with each check a `FailedTask`; `judge` reads the candidate
  timed out and `arrange` blames its newest submission.
- **Reports.** `Progress::Finished` is read for a `Testing` run only,
  but a check in `report.missing` is a failed task (`verdict.rs`), so a
  report short of checks blames. An unproven replay reopens the whole
  run with `no_replay`, every check run again.
- **Landing.** `PushedRefs::leased` reads `[rejected]` at the ref as
  `Moved::Elsewhere` and every other refusal as an error; the intent
  stays, every round pushes again and fails before `decide`;
  `withdraw_in` and `admit` refuse a `Landing` submission.
- **The pool.** `launch`, `job_alive` and `wake` reach GitHub
  (`profiles/github/actions.rs`); the fake pool runs jobs as peers named
  by the claim. No workflow, no `sluice job`, no `sluice init`; a
  `github` signer is `SignerKey::Keyless`, verified by nothing, so its
  records count for nothing.

## Goals / Non-Goals

**Goals:**

- One owner per run state, written before every act, every act provable
  and safe to find done.
- A limit that is the run's own, and a run no one may take that blames
  no one.
- A change of lead that changes no run.
- A report read only at its run's end; a check it lacks blames no one.
- Only the undecided checks run again.
- A forge's refusal held for the submitters, never pushed blind.
- The checks no standing host takes run on GitHub Actions, signed apart
  from them, verified on every host from the records alone.

**Non-Goals:**

- A round job leading a branch with every host off, the wake after a
  finished run and the schedule: `a-github-action-runs-a-queue-whole`.
- The CodeBuild pool and KMS signer of record 0119.
- In-toto's Test Result kind for the run's statement (record 0077).
- Mapping records of the older shape: a host's queues are drained before
  the release that brings this (principle 12).

## Decisions

### D1. A run's five states, in the records

`RunState` becomes `open`, `claimed`, `running`, `ended`, `closed`. The
run (`SystemRun`) keeps `system`, `checks`, `declined`, `lost` and
`no_replay`, and gains:

- `opened`: when it was opened; a status reads waiting since then.
- `claim: Option<Claim { by, at }>`, replacing `host`: the instance or
  `pool/<pool>/<n>` that holds it, and when it claimed.
- `place` and `run` as today, `place` named at the claim, `run` (the
  handle) at `running`.
- `running: Option<String>`: the run's own start, from the proof of D3.
- `ended: Option<Ended { at, outcomes, timed_out, why }>`: each given
  check's outcome, `passed`, `failed`, `unproven` or `none` with why;
  `why` for a run that ended with nothing run (a lapsed claim, a run no
  executor took).
- `closed: Option<String>`.
- `follows: Option<usize>`: the run it follows (D7).

A state moves only forward. Nothing writes `ended` or `closed` twice or
otherwise (D9). `CandidateState` loses `Starting`, which a run's
`claimed` carries, and gains `NotPassed`, a verdict with no culprit
(D6). The log gains `opened`, `running`, `ended` and `closed` per run,
beside `claimed`; `lapsed`, `lost`, `uncounted` and `unproven` lines
already written are read as history (core.41), and an `ended` line
carries their causes from now.

*Not taken:* keeping `Starting` and `Testing` with more fields. They
name what a round decided, not what was done, and the run's outcome
would stay in three verdict states besides.

### D2. Who writes what

| state | written by |
|---|---|
| `open` | the lead, as it opens the combination, or a follow-up |
| `claimed` | the claiming instance; for a pool, the lead, under `pool/<pool>/<n>` |
| `running` | the claim holder, from the supervisor's start; for a pool, the lead, from the run job's start |
| `ended` | the claim holder from its report; a pool's sign job; the lead for a lapsed claim or a run no executor took |
| `closed` | the claim holder once its checkout is released; with `ended` for a pool's run and a run that made no checkout |

On a branch in the host's own files the lead is every claim holder. A
claim holder that does not lead writes its own run's states into the
branch's records with a leased write (62), as `execute` reports today;
the lead's `poll` acts only on the runs it opened or holds.

### D3. Write, then act, and every act provable

`decide` writes; `perform` acts on what is written; a write follows each
act's proof. Each act and the proof the next reader checks:

| act | named by | proof | found half done |
|---|---|---|---|
| make the checkout | `claimed`, `place` | the place profile lists it made at the combination | locked `initializing`: undone and made again (D4) |
| start the checks | `claimed`, checkout made | the supervisor's run for the checkout (`.runs/<checkout>/`), with its start | none |
| dispatch a pool's job | `claimed` by `pool/…` | a workflow run titled `sluice <claim>` | none within the bound: a lapsed claim |
| record the results | the supervisor's word that the run ended | the results under the run's keys and combination (`same_as`) | only what is missing is recorded |
| release the checkout | `ended` | the place profile no longer lists it | held (123) |
| tell a submitter | the outcome in the outbox | weir's hand-over | as today |
| land | the landing intent | the branch holding the landing | refused by the forge: D10 |

The check seam gains `Tests::found(place) -> Option<Found { run, since
}>`: whether a run was started in the checkout and when, read from the
supervisor's own files. `perform_starts` asks it for every `claimed`
run of this host before it starts anything: found, it writes `running`
from it; not found, it makes the checkout where none is made and starts.
The stopped-round branch and `recover` no longer release a `claimed`
run's checkout; a checkout made with no start found is used, where the
host holds room for it, and otherwise the run ends with no outcome and a
follow-up holds its checks (D7), its checkout released after the end.

`launch_on_pools` writes the claim in `decide`; `perform` dispatches,
after asking the pool for a workflow run of the claim's title. Making
landing commits in `decide`, publishing a combination's ref and holding
a part of the host's bound are no acts on a run: each is idempotent or
given back when dropped.

*Not taken:* one write per act, before and after. A write per act is
twice the leased pushes on a remote branch and buys nothing a proof does
not.

### D4. A checkout git left locked mid-way is undone

`git worktree add` writes `locked` with the reason `initializing` before
the checkout and removes it once done; an add stopped part way leaves
it, and `git worktree remove --force` refuses it. `Places::materialize`
and `Places::release` (`crates/sluice/src/profiles/places.rs`) read a
checkout sluice made, by its record (core.16), from `git worktree list
--porcelain`: locked `initializing` with no add of sluice's running for
it, which the branch's lock and the checkout's own name rule out, is
unlocked, removed with `--force` and pruned, then made again or let go.
A checkout locked for any other reason is held with git's words (123).

*Family:* flume's git worktree profile reads a worktree's lock as a
person's hold and keeps it; a checkout sluice made is no person's, and
sluice undoes only git's own `initializing` lock of an add it started.

*Not taken:* `git worktree remove -f -f` for every lock, which would
unlock a lock a person set by hand.

### D5. A claim lapses only by its own holder

Every instance that holds a claim on a branch on its remote writes its
sign of life each round, the lead among them: the beat `execute` writes
moves to the round of every instance on such a branch. `poll` lapses a
claim only by `lapsed`: the holder's beat standing for `[peers] alive`
by this instance's own observation from when it first read it (73), or a
pool's job ended with no record. A new lead begins its observation when
it takes the lead, so it lapses nothing of the old lead's before a
whole bound of its own reading. A lapsed run is ended by the lead with
no outcome, `why` naming the claim, and closed in the same write, and a
follow-up holds its checks (D7). The old holder, back, finds its run
ended and writes nothing (D9), and releases its checkout under 81.

### D6. The limit is the run's, and a run no one may take blames no one

The time limit counts from `running`. The supervisor holds it as today
(rule core.10) from the run's own start, and the claim holder writes
`ended` with `timed_out` from its report; no reader writes another's
running run timed out.

An `open` run is under no limit while a live executor may take it: a
live host by its last sign of life whose system it is, whose signer the
gate trusts and that has not turned it down, or a pool the policy lists
for its system whose signer the gate trusts (87, 107). Once the
combination's limit has passed since the run's `opened` with none, the
lead ends it with no outcome, each check saying who turned it down and
why (`waited_for`) or that no host that may take it was live. `judge`
reads the candidate `NotPassed`; `arrange` drops it with no culprit, no
`FailedTask` and nothing counted toward 115, logs `ended` with the why,
and its submissions stay queued and form again. Each submission's status
says the combination was not passed and why.

*Not taken:* holding the combination open until a host comes. Record
0122 ends it at the limit, so a status says it, and a host's return
needs no special reading.

### D7. Follow-up runs

A run whose report leaves checks undecided, unproven replays or checks
with no outcome, ends with every other check's outcome kept. The lead
opens one follow-up run of the same system holding only those checks,
`follows` naming it, `no_replay` where any of them was unproven. Every
path that opens a run again today ends it this way instead: a lapsed
claim, a lost run, a start refused at the bound, a checkout not made
(record 0118), a start that failed, checks with no outcome and an
unproven replay; the follow-up of a run that ran nothing holds every
check of it. `judge` reads a
check by its last outcome over a run and its follow-ups, and reads a
fail at once, opening no follow-up beside it. Runs in a row with checks
with no outcome for 114's causes count in `not_run_runs` as today; a
lapsed claim, a start refused at the bound, a checkout not made and a run
no executor took count for nothing, as today.

*Not taken:* a follow-up only for unproven replays, as record 0122
words it, with no-outcome checks reopening the whole run. A whole run
again is what an ended run's standing (D9) rules out, and 15 reads the
passes from their keys anyway; one shape serves both.

### D8. A report only at its run's end

`Progress::Finished` is answered only once the supervisor's run is over,
its process group gone and `result.json` written; a pool's report is
the sign job's record. `poll` and `execute` read a report only for a
`running` run whose handle is its own. `verdict::read` reads a check in
`report.missing`, or a given check the report names nowhere, as no
outcome, `the run's report holds no outcome for it`, never a failed
task; a required check the runner keeps out of its CI mode, by the
runner's own word in `kept_out`, is read as today (ADR 0050).

### D9. A written end stands

Nothing rewrites a run in `ended` or `closed`. `finish_run` and
`execute` write nothing for a run whose state is not `running` under
their own claim. `choose_landing`'s reading of a passed candidate with
no signed pass behind it (design D8 of `peers`) no longer resets the
candidate to `Formed`: the runs stand, the candidate reads under test
again, and a follow-up run holds each check with no counted pass.

### D10. A landing the forge refuses is held

- `Moved` gains `Refused(String)`. `PushedRefs::leased` reads a refused
  push that is not `[rejected]` at the ref by fetching the branch:
  still at `expected`, `Refused` with the forge's lines whole
  (`refusal`); elsewhere, `Elsewhere`; holding `to`, `Moved`.
- `perform_landing` on `Refused` writes `landing.refused { words, at }`,
  each of the landing's submissions `State::Refused`, and one
  `forge_refused` line; a round with a refused landing pushes nothing
  and `decide` forms nothing for the branch. A refused submission is not
  in play, so the host process may end.
- `withdraw_in` and `admit` take a `Refused` submission as any queued
  one. `drop_withdrawn` drops a refused landing once any of its
  submissions has left, and the rest go back to `Queued`.
- The refusal is owed under `<id>:refused`, carrying the words, through
  `push_outcome`'s weir delivery. It decides no outcome.
- A wait on a refused submission ends at once with exit 6, `the forge
  refused its landing: <words>`; its document is the record.
- `sluice push <branch>` clears `refused` under the branch's lock and
  runs the round's landing once; refused again, it is held again with
  the new words, and the command exits non-zero with them.

*Not taken:* a refusal blaming its submissions, a timer pushing again,
and forming the chain again (record 0124's options 2 to 4). Exit 1, which
flume's and crew's readers take as failed.

### D11. The Actions pool: one workflow run, two jobs

`crates/sluice/src/profiles/github/workflows/` holds `sluice.yml` and
`sluice-pr.yml`; `workflows.rs` renders them with the release that
writes them. `sluice.yml` runs on `workflow_dispatch` (inputs `claim`,
`branch`, `combination`, `system`, `runner`; title `sluice <claim>`)
and on `workflow_run` of `sluice-pr.yml`. A pool's dispatch sends those
five inputs and no other, since GitHub refuses a dispatch carrying an
input its workflow does not declare. The workflow runs no round, so a
submission's wake of a GitHub pool dispatches nothing: the round job and
its wake are `a-github-action-runs-a-queue-whole`'s, and until then the
wake is lost, which only delays the submission (59). The jobs:

- **run**, on `${{ inputs.runner }}`, `permissions: contents: read`, no
  environment: installs sluice's release and the toolchain the policy
  names, runs `sluice job run`, uploads its directory.
- **sign**, `needs: run`, `if: !cancelled()`, on `ubuntu-latest`, the
  `sluice` environment, `id-token: write`, `attestations: write`:
  downloads the artifact, runs `sluice job sign`, signs with
  `actions/attest` (subjects and predicate as `sluice job sign` writes
  them), runs `sluice job record` with the bundle. A run job that wrote
  no report gets a sign job that records the run ended with no outcome.
  A cancelled workflow run signs nothing: its sign job is skipped, and
  the lead ends the run as a job that ended with no record (D12).

`sluice job` is one subcommand with `run`, `sign` and `record`.
`sluice job run` starts its run through `Sluice::start_claimed` under
the job's claim, as its doc has awaited, with the job's own host process
holding no room of a host (rule core.49). The lead writes the pool's
`running` from the run job's `started_at`, read by `Tests::job` (D12),
and the sign job writes `ended` and `closed` in one write.

`sluice-pr.yml` runs on `pull_request`: one run job per system the
pool's runners name, `sluice job run --head <sha>`, with no secret and
no `id-token`; `sluice.yml`'s `workflow_run` sign job signs the head's
proven keys as on the signer's branch, the statement's `claim` none
(113).

*Not taken:* one job per run with `id-token: write`, in which any check
could ask for the token and sign any key; the round job, which is
`a-github-action-runs-a-queue-whole`'s.

### D12. The pool's seam

`Tests::job_alive` becomes `Tests::job(kind, claim, remote) ->
Job`: `None` (no workflow run of the claim's title), `Queued`,
`Running { since }` (the run job's start), or `Ended { recorded }`.
`lapsed` reads `Ended { recorded: false }` and, past the sign-of-life
bound since the claim, `None`, as a lapsed claim. `Actions::job` lists
the workflow's runs by title, completed among them, conditionally
(core.32), and reads the run job's `started_at` from its jobs. The fakes'
pool keeps each job's state for tests to move.

### D13. Keyless verification with the roots sluice carries

`crates/sluice/src/profiles/github/keyless.rs` verifies a bundle with
`sigstore-verify` and `sigstore-trust-root` (sigstore/sigstore-rust,
pinned in `Cargo.toml`), against `TrustedRoot::from_embedded` for the
public-good instance and GitHub's: a bundle with a transparency-log
entry against the public-good root, one with an RFC 3161 timestamp and
none against GitHub's. The policy's `VerificationPolicy` is the
certificate's identity, `https://github.com/<owner>/<repo>/<workflow>@<ref>`
from `[signers.<name>] github` and the branch's remote, and GitHub's
issuer. The verified envelope's statement is read as an SSH-signed
record's is. `remote_refs.rs`' `SignerKey::Keyless` carries the
identity, `verify` calls the verifier, and `SignerKey::lists` holds for a
pool's signer, so `counts_here` lets a pool's job claim. Verification
calls no service, as 78 asks ("from the records alone"); the roots
change with the crate's release, and a bundle no carried root verifies
counts for nothing, with why.

*Not taken:* trusted roots fetched by TUF and cached on the host,
`actions-pool`'s D1, which calls a service at a read and gives the host
that can write its cache the root of trust; sluice's own x509 and RFC
3161 checks for GitHub's instance, which `sigstore` 0.14 forced and
`sigstore-verify` makes needless; `gh attestation verify`, which every
host would need.

### D14. sluice init actions and forge check

`sluice init actions` writes both workflows, appends `[signers.forge]`
and `[pools.forge]` as text where the policy, read with `toml`, has
neither, the file's own lines untouched, leaves a differing file
without `--force`, and prints the permission and the environment. The
signer's ref is the remote's default branch as the working copy holds
it, else `main`: a session runs `init` in a worktree on a feature
branch, which no signer's workflow runs from. The pool's runners are
those GitHub hosts for the systems `[systems]` names, each it hosts none
for named in the output; a policy naming no systems gains `linux-x64`. `sluice
forge check` adds, for a policy with a GitHub pool, the workflow on the
signer's branch as `init` would write it, the app's `actions: write`, the
`sluice` environment's deployment branch policy naming only the signer's
branch, and the signer's workflow and ref. The refusal of a pull request
table on records each host keeps names `sluice init actions` beside
`[signers]`.

### D15. actions-pool is archived by this change

This change's delta specs carry what `actions-pool` proposed and this
change builds, the round job aside; its last task archives `actions-pool`
with `openspec archive actions-pool --skip-specs`, so its specs are
applied once, from here.

### D16. Records and rules

Record 0122 gains a build amendment for the run's life (D1 to D9), and
record 0119 one for the Actions pool (D11 to D13). Two rules join
`docs/architecture/core.md`: core.61, a run's state written in its home
before any act, by its claim holder or its lead, its end standing; and
core.62, a pool's checks never in a job that holds the signing identity
or the app's key. core.52's details name the `<id>:refused` delivery,
and core.22's a keyless record verified against the roots sluice
carries. The numbers clear main's core.59 and this bolt's core.60.

## Looking across the family

- **flume** reads a submission's `state` from `sluice status --json`
  and submits with `--no-wait`; it meets `refused`, a word it has not
  read. flume's read of it is flume's to add; the orchestrator is told
  when this lands.
- **crew and `wt submit`** read a wait's exit; 6 is new, and none of
  them takes it as 1's failure.
- **weir** owns the delivery form (core.52) and bounded calls (core.29):
  the `<id>:refused` delivery and the forge's calls of D12 go through
  them as every other does.
- **flume's places** keep a person's lock (D4).

## Risks / Trade-offs

- [More leased writes] A claim holder that does not lead writes
  `claimed`, `running`, `ended` and `closed` on the remote. → Four writes
  a run, each small, where `execute` already writes two.
- [A host on the older release] Records of the older shape are not
  read. → Each host's queues are drained before the release, as for
  record 0122's first half.
- [A run no one may take, formed again] A combination no host can take
  is opened again every limit. → One `ended` line each limit; the status
  says why.
- [Embedded roots go stale] A root GitHub rotates after sluice's
  release leaves new bundles unverified. → Missing, never a pass; the
  next release carries it.
- [Pool cost] Every run no standing host takes is a billed job. →
  Standing executors come first (107); the guide says so.

## Migration Plan

Drain each host's queues, install the release, run `sluice init
actions` where a pool is wanted, set up the app's environment as it
prints, and land the policy that names the pool. Rollback: a policy
without the pool.

## Open Questions

None.
