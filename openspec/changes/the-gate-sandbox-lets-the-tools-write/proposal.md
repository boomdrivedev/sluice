## Why

On 2026-11-24, the gate of #160 took 17.5 minutes. Its slowest task
takes 9. `cli:test` failed in two runs, and sluice ran the run again
each time, 3 minutes for each run.

The cause is in two parts:

1. **A refusal erases a fail.** sluice's sandbox reported a refusal in
   each run: moon's name lookups, cargo's writes to its home, and
   moon's writes of its task archives. A check that failed in a run
   with any refusal had no outcome (139; rule core.35). sluice recorded
   nothing, blamed nobody, and ran the combination again. A real
   failure cost two extra runs, and its outcome was lost. The refusals
   come in every run, so every fail of a moon task was erased.
2. **The sandbox refuses what the tools do in each run.** moon asks
   public name servers whether it is offline. moon writes each task's
   archive to `.moon/cache/outputs`, which `devenv.nix` links to the
   git common directory. cargo writes its lock files in its home. None
   of these is a refusal that makes a test fail.

The two `cli:test` fails were also flaky tests:
`remote_branch a_held_landing_lands_at_the_next_round_once_the_forge_takes_it`
fails in about half its runs, and two `host_process` tests fail under
load.

## What Changes

- **A fail stands, whatever the sandbox refused.** A check that ran and
  failed is a fail: sluice records it under its key and blames its
  entry. sluice still reads what the sandbox refused, from the
  sandbox's own report, and says it beside the fail: on the run's
  `detail` line in the branch's log, in `sluice status` and in
  `sluice exec`. A check has no outcome only where it did not run or
  the host changed under it: its program did not start, it exited with
  a `not_run` code, its time limit ended it, the host's system was
  switched, the container engine went, or nothing proves its pass.
- **A run held from the network is told so.** Where the policy's
  network is `denied` or `localhost`, the run's command gets
  `PROTO_OFFLINE=1`. moon and the tools proto runs then make no name
  lookup. Equipping, outside the sandbox, gets nothing.
- **The sandbox lets moon keep its task archives.** It opens the
  checkout's `.moon/cache/outputs` where a link leads it, as it opens
  moon's store.
- **The held-landing tests step until a round runs.** A step that
  finds the host process's own round holding the branch's lock runs
  no round. The test asked once and read the state too early.
- **The host process tests that hold a time under load** are found and
  fixed, or leave the functional suite for `cli:speed` (design D5).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `run-outcome`: a fail in a run the sandbox refused something is a
  fail, and names what was refused.
- `host-caches`: a sandboxed moon run writes its task archives.
- `moon-runner`: a run held from the network is told it is offline.

## Out of scope

- **cargo's home.** cargo writes `.package-cache`, `.global-cache` and
  extended attributes under `~/.cargo`. The writes fail and cargo goes
  on, so no test fails for them. The host declares a folder the
  sandbox lets a run write, as one of its `[[caches]]` (35). On deneb
  that is swancloud's `lib/host-caches.nix`, a change of swancloud's.
- **Tests that reach the host's own state.** Under the sandbox, tests of
  `checks`, `runs`, `telemetry` and `usage` start processes that write
  `~/.local/state/sluice/running/.let-go` and
  `~/.local/state/sluice/host/start.lock`, and `host_process` tests make
  name lookups through git. The writes are refused and the tests pass.
  They wait on a change of their own: a run's supervisor writes
  `.let-go` to the host's registry, not to the registry that counted
  the run.

## Impact

- `crates/sluice-core/src/step.rs` (`after_run`),
  `crates/sluice-core/src/records.rs` (`Event::Detail`),
  `crates/sluice-core/src/session.rs`, `crates/sluice-seams/src/lib.rs`
  (`RunReport::refused`).
- `crates/sluice/src/profiles/checks.rs` (`sandbox_profile`,
  `offline`), `crates/sluice/src/profiles/moon.rs` (`archives`),
  `crates/sluice/src/exec.rs`, `crates/sluice/src/lib.rs` (status).
- Tests: `crates/sluice-conformance/tests/could_not_run.rs`,
  `failed_dependency.rs`; `crates/sluice/tests/exec.rs`,
  `remote_branch.rs`, `host_process.rs`.
- Docs: `docs/architecture/core.md` (core.7, core.35, core.37), the
  site's `reference/configuration.md`, `reference/commands.md` and
  `reference/state.md`, and the using-sluice skill's `verdicts.md` and
  `exec.md`.
- Decisions: proposed in Boom (design D1 to D3).
- Sources: clauses 35, 84, 114, 139, 149, C.2; rules core.7, core.34,
  core.35, core.37; record 0072.

## Touches

```
crates/sluice-core/src/step.rs
crates/sluice-core/src/records.rs
crates/sluice-core/src/session.rs
crates/sluice-seams/src/lib.rs
crates/sluice-fakes/src/lib.rs
crates/sluice/src/profiles/checks.rs
crates/sluice/src/profiles/moon.rs
crates/sluice/src/exec.rs
crates/sluice/src/lib.rs
crates/sluice-conformance/tests/**
crates/sluice/tests/exec.rs
crates/sluice/tests/remote_branch.rs
crates/sluice/tests/host_process.rs
docs/architecture/core.md
site/src/content/docs/reference/**
plugins/sluice/skills/using-sluice/references/**
openspec/changes/the-gate-sandbox-lets-the-tools-write/**
```
