# Design

## Context

timber 135 has a pool's job push the cache entries of the tasks it ran
"as a step of the repository's own workflow after the queue's run step,
which the queue's template leaves room for and never names, with a
credential of the job's own that writes cache entries and nothing else
in the store". Which entries, timber reads from the checkout the run
left (timber 139, *left*: "given a checkout, the build identities whose
outputs the checkout's own build state holds"). So the step must run in
the checkout the checks ran in, before anything removes it, with a
credential sluice neither holds nor names.

sluice's two pools, as record 0119 and its amendment of 2026-11-16 built
them, leave no such place:

- `sluice job run` makes the claimed run's checkout, runs its checks,
  keeps the hand-over (`report.json`, `proof/`, `why`) in `--out`, and
  releases the checkout before it exits (`crates/sluice/src/job.rs`,
  rule core.63). Its writes to the branch's records are held in memory
  and sent nowhere (`Unwritten`), so the place it made is known only to
  that process.
- `sluice.yml`'s run job (`crates/sluice/src/profiles/github/workflows/`)
  checks out the signer's branch, installs Nix and sluice, runs `sluice
  job run` and uploads the hand-over. `sluice-pr.yml`'s run job does the
  same for a pull request's head. Both hold `contents: read` and nothing
  else.
- The CodeBuild run project's buildspec
  (`crates/sluice/src/profiles/codebuild/templates/stack.yml`) cleans the
  instance, installs, clones with `SLUICE_READ_TOKEN`, and runs `sluice
  job run` in its build phase; CodeBuild uploads `sluice-run/` as the
  hand-over after every phase. The run role may write the hand-over and
  its logs alone.
- `sluice init actions` and `sluice init codebuild` leave a file that is
  not byte for byte what they write unless `--force`, which writes
  theirs over it. `sluice forge check` reads `sluice.yml` on the
  signer's branch, and `sluice pool check` each run project's buildspec,
  byte for byte against what this release writes.

## Goals / Non-Goals

**Goals**

- A team adds its own steps to each pool's run job or run build, after
  the checks, and keeps them through `sluice init … --force` and every
  reading of the setup.
- Those steps run in the checkout the checks ran in, which the job
  releases after them, whatever they came to.
- The repository gives them a credential of its own; nothing the
  repository adds there gives the run job or the run build an
  environment, an identity token, the app's key or the signing key
  (core.62).
- sluice names no store and no tool in what it writes (timber 135;
  core.1, core.26).

**Non-Goals**

- sluice pushing, or knowing what is pushed.
- Ending what the checks left running before the repository's steps:
  it ends at the release, as on a host (core.10).
- A store on GitHub's registry from a CodeBuild pool, which GitHub
  gives no token for (timber C.1).

## Decisions

### D1. The room is two `# sluice:` lines in each run job and run build

Every run job sluice writes, `sluice.yml`'s `run` and `sluice-pr.yml`'s
`run`, carries, among its steps and at their indentation, the two lines

```yaml
      # sluice: your own steps, after the checks, in $SLUICE_CHECKOUT
      # sluice: end of your own steps
```

and each run project's buildspec in the CodeBuild template carries,
among its `post_build` phase's commands and at their indentation,

```yaml
      # sluice: your own commands, after the checks, in $SLUICE_CHECKOUT
      # sluice: end of your own commands
```

The lines between a pair are the repository's own steps or commands.
sluice writes the pair with nothing between, reads nothing of what is
between but that it stays there (D3), and carries it over when it writes
the file again. The words are the reader's (core.26): the comment tells
an operator who opens the file what goes there.

*Not taken:* a local composite action the run job calls where the
repository holds one. A composite action reads no `secrets`, so the
credential would have to come through inputs the template would have to
name, and its file would be read from the checkout of the signer's
branch, a second file to keep beside the workflow. *Not taken:* a
command the policy names, which `sluice job run` runs before its
release. It would put the repository's credential in the environment of
the process that runs the checks, and timber 135 asks for "a step of
the repository's own workflow", which the policy is not.

### D2. The checkout is kept for the room and released after it

`sluice job run` takes `--checkout <file>`, which every caller gives.
Where it made a checkout and was not stopped, whatever its run came to,
it leaves the checkout in place, with its ports and scratch, and writes
`<file>`: the checkout's path on its first line and its place's name on
its second. Where it made none, it writes no file. Where it is stopped,
by Ctrl-C or SIGTERM, it releases the checkout at once, as it does
today, and writes no file.

A new `sluice job release --checkout <file> [--out <dir>]`, run in the
clone `sluice job run` ran in, releases the place `<file>` names, by the
release the policy names, its ports and scratch freed, and removes
`<file>`. A release that fails is said in its output, appended to
`<dir>/why` where `--out` is given, and exits 1; it decides nothing of
the run's outcome (core.63). Where `<file>` does not exist, it releases
nothing, says so and exits 0.

Each template names the kept checkout to the room as `SLUICE_CHECKOUT`,
from `<file>`'s first line, wherever `sluice job run` kept one:

- **GitHub Actions.** The run job runs `sluice job run --checkout
  "$RUNNER_TEMP/sluice-checkout"`; a step after it, `if: ${{
  !cancelled() }}`, appends `SLUICE_CHECKOUT=<path>` to `$GITHUB_ENV`
  where the file holds one; then the room; then `sluice job release`,
  `if: always()`; then the hand-over's upload, `if: always()`, as now.
  GitHub runs an `always()` step after a cancel too, so the job
  releases its checkout when someone cancels the workflow run during the
  repository's steps.
- **CodeBuild.** The build phase runs `sluice job run --checkout
  "$CODEBUILD_SRC_DIR/sluice-checkout"` as its last command. The
  `post_build` phase, which CodeBuild runs whatever the build phase came
  to, exports `SLUICE_CHECKOUT` and the `PATH` sluice is on, then holds
  the room; its `finally` block runs `sluice job release --out
  "$CODEBUILD_SRC_DIR/sluice-run"` in the clone, which CodeBuild runs
  whatever the room's commands came to. CodeBuild uploads the hand-over
  after `post_build`, so a release's why is in it. The first command of
  the build phase also removes `$CODEBUILD_SRC_DIR/sluice-checkout`
  beside the scratch and state an earlier build left (core.63).

The hand-over is kept by `sluice job run` before the room runs, and
uploaded after it on both pools, so the release's why reaches the sign
job on both. The room's steps could change the hand-over before its
upload, as the combination's checks could change what they report:
record 0119 has a run job's outcomes as the combination's word, and the
sign job proves only the keys, from the combination's tree (core.62).
The room gives the hand-over no trust it lacked.

*Not taken:* `sluice job run` releasing at its exit unless a flag keeps
the checkout, so a hand-run job behaves as before. Every caller is a
template sluice writes, and both leave the room, so a second behaviour
would serve no caller (build the design, not the transition). *Not
taken:* `sluice job release` finding the place in the branch's records
by its claim. `sluice job run`'s writes are held in its own process and
sent nowhere, the run may be closed or finished by then, and a head run
has no claim.

### D3. Setting up keeps the room, and every reading empties it

One reading, in a new module `crates/sluice/src/own_steps.rs`, serves
the four commands that compare a file with what sluice writes: given the
file as it stands and as sluice writes it, it answers that the file is
as sluice writes it, with or without the repository's own steps; that
it differs, naming the first line; or that the repository's own steps
leave their place, naming the line.

- **By its text.** Each room's lines, the lines between a pair of D1's
  lines, are taken out, and the rest is compared with what sluice writes
  line for line. A file missing a line of a pair, or holding one twice,
  differs.
- **By its parse.** The file is parsed as YAML, as is what sluice
  writes; with the room's entries taken out of the run job's `steps`, or
  the `post_build` phase's `commands` in a buildspec, the two parses
  are equal. Where they are not, the room's lines reach beyond the
  steps, as a line less indented than the pair, or a string opened in
  the room and closed in sluice's lines, would, and the room leaves its
  place. So nothing a room holds can add a key to the run job, such as
  `environment:` or `permissions:`, or to anything else in the file.
  `yaml-rust2`, already pinned for the template's tests, moves to the
  crate's dependencies.

The module names no tool and no forge, so it belongs to no profile, and
the GitHub and CodeBuild profiles each call it with their own pairs and
their own path to the list (core.1, core.2).

The commands that use it:

- **`sluice init actions`** reads each workflow by it. A workflow as
  sluice writes it, with or without the repository's own steps, is said
  so and left. One that differs is left unless `--force`, as now;
  `--force` writes sluice's file with each room's lines carried over
  into the same job's room. One whose repository's steps leave their
  place is left, said with the line, and exits 1, `--force` or not,
  since carrying them over would carry the line that leaves.
- **`sluice init codebuild`** reads the template the same way, a room
  for each run project, carried over by its system; a room of a system
  the template no longer writes is said, by its system, and not carried.
- **`sluice forge check`** reads `sluice.yml` on the signer's branch by
  it: as sluice writes it, with the repository's own steps said, passes;
  one whose repository's steps leave their place fails, naming the
  line.
- **`sluice pool check`** reads each run project's buildspec by it, the
  sign project's byte for byte as now, since the sign build has no room.

### D4. On GitHub Actions, the repository's steps hold the repository's credential

A step in the room names the repository's own secrets, `${{ secrets.X
}}`, as timber's guide does for a key that writes the store's cache and
nothing else. The `sluice` environment's secret, the app's key, stays
out of reach: the run job names no environment, and a room cannot add
one (D3).

The run job's `permissions` are sluice's `contents: read`. The
repository may add `packages: write`, the line directly after it in the
run job's `permissions`, for a store on GitHub's registry, where timber
C.1 has "the pool's token is the job's own, given write on its
repository's cache packages and nothing else". D3's reading takes that
one line out with the room's, and its parse accepts it in the run job
alone; `--force` keeps it. Any other permission, `id-token`,
`attestations`, `contents: write` among them, makes the file differ.

The token with `packages: write` is reachable by the checks too, since
the job's token is in the job's checkout's git configuration. A team
that adds it keeps its other packages out of the repository's Actions
access, as timber C.1 has the publishing host make its component
packages; sluice cannot see a package's access, and says so in the
guide.

*Not taken:* `packages: write` in every run job sluice writes. A
repository whose workflows publish container images would hand every
check write on them. *Not taken:* `sluice init actions --packages`. A
flag decides once what the file then says on its own; the line in the
file, kept by `--force`, is the decision.

### D5. On CodeBuild, the repository gives the run role its policies, and the role denies itself sluice's keys

The template takes a parameter, `RunPolicies`, a comma-separated list
of managed policies' ARNs, empty by default, which the run role is given
where it names any (a condition on the parameter, so an empty list adds
none). A team gives it a policy that writes its store's cache and
nothing else, as timber's guide gives its IAM user.

The run role's own policy gains explicit denies, which no policy it is
given can lift (IAM reads an explicit deny over any allow): every `kms:`
action on the signing key, every `secretsmanager:` action on the app's
key, every `codebuild:` action and every `iam:` action. So whatever the
repository gives the role, a run build cannot sign, read the app's key,
start or change a build, or change its own role (core.62). The key's
policy already lets the sign role alone sign.

`sluice pool check` reads nothing of the role's policies: the lead role
may read no IAM, and sluice cannot judge what a policy lets a store do.
The guide says what the policy should hold, naming no tool.

*Not taken:* a secret of the repository's in Secrets Manager, read into
the run project's environment. It would put a key, kept for good, where
the role's own credential, which lapses with the build, does the work,
and the template would name the secret.

### D6. `sluice-pr.yml` leaves the same room

A pull request's head run is a run job of the pool's (113), and what it
built is as much worth pushing. Its run job leaves the same room, keeps
and releases its checkout the same way and takes the same `packages:
write`. GitHub gives a pull request from a fork no secrets and a token
that reads; that is GitHub's, and the guide says so.

### D7. What the guides say

The pools guide gains, under each pool, how to add the team's own steps:
where they go, the checkout they run in, the credential they hold and
what it should be allowed, and that they run whether the checks passed
or not where their own `if:` says so. It names no tool (core.26). Its
"What a workflow run does" and "What a run does" say the job releases
its checkout after those steps. The commands reference gives `sluice
job run --checkout` and `sluice job release`; the setting-up skill
mentions the room where it offers a pool.

### D8. Records and rules

Record 0119 gains an amendment of 2026-11-18, this change's choices,
which are its own and cheap to change, holding no choice of the user's:
D1 to D6. Rule core.62's details: a run job given an environment,
`id-token: write`, an attestations grant, a secret of sluice's or any
permission but `contents: read` and the repository's `packages: write`
is ruled out; a room's line that would add any of them is ruled out; a
run build's role without its denies, or given a policy past them, is
ruled out; the repository's own steps in the room, naming the
repository's own secrets, and the run role given the repository's own
policies are not ruled out (timber 135). Rule core.63's details: a pool's
job ending with its checkout held is ruled out, as is `sluice job run`
stopped exiting with it held; `sluice job run` keeping its checkout for
the job's own steps, which `sluice job release` releases after them,
whatever they came to, is not ruled out.

## Looking across the family

- **timber** owns the push (135) and what a run left (139). Its guide,
  *The run job's push*, must move its step between D1's lines and run it
  in `${{ env.SLUICE_CHECKOUT }}`, not `${{ github.workspace }}`, which
  is the checkout of the signer's branch and holds none of what the
  checks built. It already keeps the run job free of an identity token,
  as sluice does. timber C.1's "pool's token" on GitHub's registry is
  the run job's `packages: write` of D4. Both are timber's to change; the
  orchestrator is told.
- **The statement.** No clause of sluice's stops holding: C.3's launched
  row still reads the job's start as its claim and its end as its
  report, and the room runs between them. timber 135 is the clause the
  room answers, and sluice cites it rather than restating it.
- **Boomdrive, Flume, weir**: none of them reads a pool's job; nothing
  of theirs changes.

## Risks / Trade-offs

- **A check that outlives its run can read the room's credential.**
  What the checks left running stays until the release (core.10), as the
  same user, so it can read the room's environment. The credential's own
  scope bounds it, writing cache entries alone, as timber 135 designed
  for "no job that builds".
- **A formatter or linter could move a `# sluice:` line.** yamlfmt and
  actionlint run over the workflow templates and cfn-lint over the stack
  (core.21); the templates' tests read the pairs where D1 puts them, so
  a formatter that moves one fails a test, not a reading in the field.
- **A workflow written before this change loses steps added after
  `sluice job run` on `--force`.** `sluice init actions` says the file
  differs and names the first line; the guide says to move such steps
  into the room before forcing. No tool moves them (build the design,
  not the transition).
- **CodeBuild's shell between phases.** D2 relies on the `post_build`
  phase and its `finally` block running after a failed build phase and
  a failed command, and exports what the room needs again rather than
  leaning on the build phase's environment.

## Migration Plan

Install the release; run `sluice init actions --force` or `sluice init
codebuild --force`, moving any step added after `sluice job run` into
the room first; land the workflow, or deploy the stack again with
`RunPolicies`; `sluice forge check` or `sluice pool check`. Rollback:
the earlier release's `--force`, which drops the room.

## Open Questions

None.
