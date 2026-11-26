## Why

On 2026-11-26, boomdrive's `main` held #40 under test for more than 30
minutes, and #42, which passed, waited behind it. `sluice status main`
showed the run of `main + #40` as "starting on deneb".

sluice formed #40 with #39 on `7461f85`. That run replayed
`boomdrive-hosted:cloud-protocol-checks`, and no counted pass proved the
replay (108). sluice opened a follow-up run that held only that check,
with no replay. #39 then landed as `1ef89db` on its own combination, so
the combination of #39 and #40 became `main + #40` on `1ef89db` (17).

Each run reads what its combination changed since the branch's revision
(36, 70). For the follow-up, that revision was now `1ef89db`, and #40
changed only `CLAUDE.md`. The check was not affected, so moon got no
target. The run's report held no outcome for the check, so sluice opened
another follow-up. This continued with no end: 28 runs in 30 minutes,
each about 20 seconds to make its checkout and none of it running the
check.

## What Changes

- A follow-up run runs each check that it holds. It does not read again
  what its combination changed. The run that it follows chose those
  checks by its own reading, and the branch can move under the
  combination after that reading (17).
- A run of the gate's checks reads what its combination changed, as
  before.

## Capabilities

### Modified Capabilities

- `run-life`: a follow-up run runs every check that it holds.

## Impact

- `crates/sluice-core/src/step.rs`: `start_run` gives a follow-up run
  that holds checks of its own no revision to read changes since.
- `crates/sluice-conformance/tests/run_life.rs`: the test of a follow-up
  that starts after the work ahead lands.

Out of scope: a bound on follow-up runs whose report holds no outcome
for a check. With this change, sluice opens no such run for a check that
the follow-up did not run.
