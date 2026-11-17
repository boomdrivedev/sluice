---
number: 120
title: A worktree sluice holds nothing for is released by its policy's release command
status: accepted
date: 2026-11-11
decision-makers:
- wldn-design's ruling, switchboard-kit PR 522 (d53448f57), relayed by agentplot-operator
- the planner's brief of 2026-11-11
- the construct of an-unheld-worktrees-release-runs-its-release-command
---

# A worktree sluice holds nothing for is released by its policy's release command

Status: accepted. The session-runs-1 proposal's clause 95, rule core.10 and wldn-design's ruling decide it: `sluice release --worktree` runs the policy's release command in any worktree of the repository, held or not. The choices within it, which policy is read (D2), what the release runs with (D3), what it keeps (D4), its answer (D5) and which folder it releases (D6), are the construct's own, each cheap to change.

Deciders: wldn-design's ruling in switchboard-kit PR 522, commit d53448f57 (`docs/architecture/testing.md`), relayed by agentplot-operator on 2026-11-11 with its form, one path in sluice's release and one test; the planner's brief of 2026-11-11; and the construct of the change `an-unheld-worktrees-release-runs-its-release-command`, which chose the form below.
Sources: signal 2026-11-11-agentplot-planner-b1e1f248/01 (afterthought/crew-state, `agentplot/main`); switchboard-kit PR 522, d53448f57, and its `infra/ministack/release.sh`; the user, 2026-11-11, refusing a sweeper on proposal 1128: "This is the whole point of having Sleuth with its release command."; agentplot/blueprints `design/sluice/proposals/session-runs-1.md` (origin/main ad3a1ae) clauses 94 and 95, C.1's session worktree row, C.5's worktrunk row and C.7; `design/sluice/requirements.md` clauses 25, 34 and 56; sluice at 49fcf7d3: `crates/sluice/src/exec.rs` `release`, `crates/sluice/src/profiles/places.rs` `release_session`, `crates/sluice/src/profiles/checks.rs` `release_request`, `crates/sluice-core/src/session.rs` `release_policy`; records 0018, 0036, 0063, 0081.

## Context and problem statement

`sluice release` in a worktree sluice holds nothing for runs no release command. A worktree is unheld when no `sluice exec` ever ran in it, or when its place was already released. `exec::release` asks the place seam whether it holds the worktree, and where it holds none it says `nothing to release`, `{"released":false}`, exit 0. It never reads the policy and never reaches `Checkout::release_session`.

Its own help says otherwise: "Release this worktree: run the policy's release command, end what its runs left running, free its ports". So do clause 95 and C.1's session worktree row: a session's place is released by the policy's release command run in it. Rule core.10 rules out releasing a place without running its release command.

The callers read that answer as released. crew's tidy runs `sluice release --worktree <place>` and then `wt remove`; Flume's teardown of a place is `sluice release`; `wt submit` runs it before worktrunk removes the worktree. Each removes the worktree once sluice answers 0. In switchboard-kit a unit's tests started Floci from the worktree's own lease, not under `sluice exec`, so sluice held nothing for those places, and four Floci containers outlived places removed from 2026-11-07 to 2026-11-09 on one host. wldn-ops reproduced it on a throwaway worktree with sluice 0.78.1: "`sluice release --worktree <w>` on a worktree sluice holds nothing for runs no release command and answers 'nothing to release' with exit 0". wldn-design ruled that "the fix is sluice's, and it is passed to sluice's owners", and that the kit adds no second caller, "neither a worktree-removal hook nor a sweeper".

## Considered options

1. One path through `release_session`: an unheld worktree is a place for its release's length, with a record and scratch of the release's own.
2. A second path in `exec::release` that builds and runs the release command itself for an unheld worktree.
3. A sweeper that finds what removed worktrees left running.
4. Each caller, crew's tidy, Flume's teardown and `wt submit`, running the release command itself.

## Decision outcome

Option 1.

- **D1. One path.** `exec::release` resolves the worktree, its session's place name and the policy, then calls `Checkout::release_session(name, policy)` for a held and an unheld worktree alike, and one tail turns the seam's answer into words and an exit. In the place profile, a name with no record whose folder is a worktree of the repository is released through the held path: any runs left under the name are ended, then a session record is written for it under the records lock, holding the worktree, the policy's branch, a scratch from `new_scratch` and no ports block. The scratch is made and the release goes on as for a held place, through `session_place`, the check seam's builder and `remove_all_with(Fresh(request))`, then `let_go`. A builder that gives no release lets the record go and answers `Released::Nothing`. The record names the scratch while it exists (rule core.30), so a concurrent round's sweep leaves it be, and gives the release the same `SLUICE_PLACE` a `sluice exec` in that worktree is given.
- **D2. The policy read is the default branch's.** sluice holds nothing that names the branch an unheld worktree was taken off (C.7), so it reads the repository's default branch, the branch `HEAD` names, at its current revision (56), as the overview does where no branch is named (record 0036). This is the change's reading of C.7's "branch it was taken off" where sluice holds nothing that names it.
- **D3. What the release runs with.** The command runs at the top of the worktree under the executor the policy's limits choose and enters the toolchain the policy names, alone (rule core.46): the work that started what the release ends equipped the working copy. Its environment is a held worktree's, `SLUICE_CHECKOUT`, `SLUICE_TMP` and `SLUICE_PLACE`, with no `SLUICE_PORT_*`, since sluice gave the worktree no port.
- **D4. It keeps nothing.** Whether the release passes or fails, the profile lets go of the record and scratch it made for it. A held worktree's failed release keeps its place held, since its ports and runs are what sluice holds (123); an unheld one had none before and has none after. Its failure says `could not release <worktree>: <what failed>`, then that sluice holds nothing for it and `sluice release` runs its release command again.
- **D5. The answer is a held worktree's.** `released <worktree>: …`, `{"worktree","released":true}`, exit 0, when the command passed; the failure, `{"worktree","released":false,"failed"}`, exit 1, when it failed; `nothing to release`, exit 0, only where the policy names no release command or the folder is in no worktree of the repository. A default branch or a policy that cannot be read fails, exit 1, naming the branch and its refusal, and so does a `HEAD` that names no branch; either keeps the caller from removing a worktree whose release never ran.
- **D6. Which folder is released.** `--worktree` as `sluice exec` names it where sluice holds a place for it; otherwise the top of the git worktree that holds it, so a release from a folder within a worktree releases the worktree, as held where sluice holds a place for that top, under its own branch's policy. A record written for that top between the release's first read and its own record is released as held too. A folder in no worktree, a folder that has gone, and a worktree of another repository have nothing to release; the profile releases a recordless name only where its folder is the repository's main worktree or a linked worktree git lists.
- **D7. The seam.** `ReleasePolicy` gains `branch`, the branch its policy was read from, which `Sluice::release_policy` fills and the profile writes on the release's record. `Checkout::release_session`'s contract and `Released::Nothing`'s doc name the recordless case, and the fake place seam keeps the same contract. Rules core.1 and core.2 hold.

Not taken:

- **Option 2.** A second path beside `release_session`, which the ruling's one path rules out, and a second copy of the builder's call.
- **Option 3.** The user refused it on proposal 1128: "This is the whole point of having Sleuth with its release command."; the ruling keeps the kit from adding one.
- **Option 4.** The ruling keeps crew's tidy as it is, "since it already calls the one stop"; each caller running the command would be three copies of the release.
- **Adopting the worktree with `adopt`, then releasing it,** which draws a ports block the release never needed and hands the release ports no run of the worktree ever had.
- **A release with no record, or with no scratch.** A scratch nothing names breaks rule core.30 and the sweep could remove it under the running command; with no scratch, `SLUICE_TMP` and a sandboxed release's `TMPDIR` would name nothing.
- **The policy at the worktree's own head,** which rule core.15 rules out; **the target of its last submission,** which a worktree that never submitted lacks; **a branch argument to `sluice release`,** which the callers would have to pass; and **the branch the worktree has checked out,** which is no branch sluice lands on.
- **Keeping a failed unheld release's record, so `sluice status` lists it,** since a later `sluice exec` would adopt it with no ports block and `--accept-failure` would free a hold sluice made only to show a failure.
- **Passing the command's own exit code through,** since a held and an unheld worktree would answer differently for one failure, and a code the command chose could collide with sluice's own 2 and 3.

### Consequences

- Good: crew's tidy, Flume's teardown and `wt submit` change nothing, and a worktree's release command now ends what its own commands started before the worktree is removed.
- Good: a second `sluice release` in a worktree runs the release command again, so a release that failed can be asked again.
- Neutral: sluice's own policy names no release command, so its own places still answer `nothing to release`.
- Bad: a caller whose worktree's release command fails now gets exit 1 and keeps the worktree, where before it got 0 and removed it; the words name what failed.
- Bad: every unheld release in a repository whose policy names a release command enters `[tests] toolchain` first, as a held one's always has.
- Bad: a worktree off a branch whose release command differs from the default branch's is released by the default branch's; a caller naming the branch is a change of its own.

## Amendment, 2026-11-17: a release with nothing to free makes nothing

Status: accepted on 2026-11-17 as the code stage's choice within D1 and D5 above, on the brief of the fix `main--a-release-with-nothing-to-free-creates-nothing`, cheap to change.

Deciders: the brief of the fix `main--a-release-with-nothing-to-free-creates-nothing` ("A release that finds nothing to free reads only, makes no folder, and succeeds."), and its code stage, which chose the form below.

Sources: session-runs-1 clause 95; clause 35 as amended; rules core.2, core.10 and core.30; deneb, 2026-11-17: flume's tool test s107 kept a place because its teardown's `sluice release`, sluice 0.95.1, answered `creating /private/tmp/sluice-501/<key>: Operation not permitted` under a sandbox that lets it write only the place, its scratch and its declared caches, where 0.93.2's release made nothing there; `crates/sluice/src/profiles/places.rs` (`release_unheld`, `ReleaseNamed`), `crates/sluice/src/profiles/checks.rs` (`names_release`) and `crates/sluice/src/profiles/mod.rs`.

### Context

D1 wrote the unheld release's record and made its scratch before the check seam's builder said whether the policy names a release command, and let both go when it named none. Making the scratch makes the user's directory and the repository's under it (35 as amended; rule core.30), so every `sluice release` in a worktree sluice holds nothing for, under a policy with no release command, wrote outside the worktree: it took the records lock, wrote and removed a record under `<common>/sluice/`, and left the repository's directory under `/tmp/sluice-<uid>/`. A caller whose sandbox allows only its place was refused, and its release failed where it had nothing to free.

### Decisions

- **D8. Nothing to free reads only.** The place profile asks whether the policy names a release command before it takes the records lock, writes a record or makes a scratch, and where it names none answers `Released::Nothing` with nothing made: no record, lock, scratch or folder, under the repository's git directory or the user's. `exec::release` then says `nothing to release`, exit 0, as D5 says.
- **D9. The check seam answers it, through the binding.** Whether `[tests]` name a release command is the check profile's reading, `checks::names_release`, the same reading its builder makes first. The binding hands it to the place profile beside the builder, `Places::with_release(named, builder)` (rule core.2), so the two cannot disagree on which policies name one.

Not taken:

- **The place profile reading `[tests] release` itself,** a second copy of the check profile's reading in another profile (rule core.2).
- **Building the release request before the record and scratch,** which would enter the toolchain before the scratch it is given exists and before a record names it (rule core.30).
- **Deciding in `exec::release`,** which would read a check profile's key outside a profile (rule core.1).

### Consequences

- Good: a caller under a sandbox that allows only its place releases a worktree sluice holds nothing for, under a policy with no release command, as 0.93.2 released it.
- Neutral: a policy whose `[tests] release` is malformed fails the unheld release before anything is made, with the words the builder gave after making them.
