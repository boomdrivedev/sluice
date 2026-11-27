# Proposal

## Why

sluice has 2,293 tests. 953 of them are in files that start the real `sluice` binary, and about 850 start a real host process. The person, on 2026-11-27: "that is way too many fucking tests. and terrible design", and "fix it. fix the fucking testing disaster."

Most of these tests prove logic: how a round reads a pull request, how a wait follows a submission, how a router picks a remote. A unit can prove that logic over the seams and their fakes. A test that starts the binary for it starts a host process, git and often a fake HTTP server. Each such test costs seconds, and the gate runs hundreds of them on a host that also runs the agents' work.

The ruling holds for each repository with the same shape. This change makes it true for sluice.

## What Changes

- **Units test the logic.** A unit test is in the crate that holds the logic. It goes through the crate's seams and the fakes of `sluice-fakes`. Where time matters, it runs on a paused clock. A unit test starts no process, no git and no network.
- **A small end-to-end set runs the real binary.** It has one test for each command's main path, and one for each part of the host process's life. Each seam with a real tool has one contract test. The tools are git, moon, worktrunk, the sandbox, the container engine's socket, the forge's fake over HTTP and the collector. design.md names each test of the set. The set has 47 tests.
- **Each end-to-end test owns its processes.** It ends each process that it starts. nextest's test group limits how many tests of the set run at one time.
- **A test that proves what another test proves goes.** Each behaviour that a test proves today stays proven by exactly one test. `moves.md` holds the table: each test that starts a process today, and the test that proves its behaviour after this change.
- **The seams carry what the logic needs.** Where logic reaches a tool directly, the logic takes that tool through a seam. The production code then uses the seam, and the fakes implement it. The pull request driver is the largest case: it fetches and pushes through `LocalRefs` and `PushedRefs`, outside the seams. No mode or flag exists only for tests.
- **The gate's time and its peak count of processes fall.** design.md holds both, measured on deneb before the first step and after each step.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `test-children`: a unit test starts no process. The end-to-end set has a bound, and each of its tests ends the processes that it starts.

## Out of scope

- **Other repositories.** The ruling holds for Flume and Boomdrive too. Each repository makes it true in a change of its own. The Boom rule binds them all.
- **The herdr plugin's shell tests.** They test a shell plugin against a fake herdr, and start no `sluice` binary. The change `the-gate-runs-in-five-minutes` made them fast.
- **`e2e/check.sh`.** It drives a whole repository through `wt submit`, and it is the `check-e2e` tier. It stays as it is.

## Impact

- `crates/sluice/tests/`: each file that starts a process moves to in-process tests, keeps its end-to-end tests, or goes.
- `crates/sluice/src/profiles/github/pulls.rs`: fetches and pushes through a seam.
- `crates/sluice-seams/src/lib.rs`, `crates/sluice-fakes/src/lib.rs`: the seams and fakes that the moved tests need.
- `crates/sluice/moon.yml`, `.config/nextest.toml`: the `tools` task runs the end-to-end set, and a test group bounds it.
- Claims: the rule `logic-is-tested-as-units-and-few-tests-run-the-binary@3131d0f94b83`, proposed for the whole system. Sources: `a-leaked-child-fails-its-test@9a82a5147ba6`, `the-runtime-flavour-is-chosen-in-main-and-tests-pause-tokios-clock@aed40c61e3b7`, `a-gate-test-asserts-no-real-time-bound@9698ec61dc28`.

## Touches

```
crates/sluice/tests/**
crates/sluice/src/**
crates/sluice-seams/src/**
crates/sluice-fakes/src/**
crates/sluice-conformance/**
crates/sluice/moon.yml
crates/sluice/Cargo.toml
.config/nextest.toml
openspec/changes/sluice-tests-its-logic-as-units/**
```
