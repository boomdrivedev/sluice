## Why

On 2026-11-27, the gate failed #176 on
`a_release_failing_after_the_landing_leaves_the_submit_landed`. The
waiting submit landed and exited 0, but it did not say the failed
release of its checkout. The test passes when it runs alone. The gate
also failed #178 on
`a_run_is_ended_at_its_limit_after_the_step_that_started_it`, which
passes alone too.

The causes:

1. A wait's step reads the records directly. It can read them in the
   middle of the pass that lands the submission. Then the landing is
   written, and the release of the checkout is not: that pass tries it
   next.
   The step then ends the wait before the pass ends. The pass's report,
   which holds the failed release, reaches no one.
2. A waiting submit opens its subscription only after its submit. A pass
   that begins between the two does not report to it.
3. The first look of a wait drops the reports that came with it.
4. The test of a run ended at its limit steps once `result.json` exists.
   A run is finished only once its supervisor lets go of it, which comes
   after `result.json` (record 0122, design D8). Under load, the step
   read the run as still going.

## What Changes

- Each pass's report carries its number. `sluice host` sends the report
  to the followers before it marks the reading ended.
- A wait's step that ends the wait waits for the passes in flight to
  end. It then names the last pass whose report went to the followers.
- A waiting submit opens its subscription before it submits, and waits
  until `sluice host` holds it. It says the reports that came before its
  first step, and the reports up to the step's pass before its end.
- A look keeps the reports that it reads, and the wait says them.
- The test of a run ended at its limit waits until the run reads
  finished.

## Capabilities

### Modified Capabilities

- `submission-wait`: a waiting submit says what the pass that decided it
  released.

## Impact

- `crates/sluice/src/host_process/tasks.rs`: the passes' numbers, and
  `settled`.
- `crates/sluice/src/host_process/reads.rs`: the wait's step names its
  pass.
- `crates/sluice/src/host_process/protocol.rs` and `pass.rs`: the
  `pass` field of an output and of a report, which an older client and
  an older host pass over.
- `crates/sluice/src/main.rs`, `lib.rs` and `subscriber.rs`: the
  waiting submit follows before it submits, and says each report.
- Tests: `landed.rs` and `profiles/conformance.rs`.
