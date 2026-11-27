# Tasks

Work in this change's place, on its branch cut from `main`. Run a suite only when the orchestrator says that the host is free, and one suite at a time. Run only this change's projects' moon tasks, through `sluice exec main -- moon run <task>`, then submit. Each step lands alone. After each landing, write the step's row in design.md's Measurements, and fill the step's rows of `moves.md`.

Each step moves one file, the largest first. In each step, each test of the file goes to the place that design D1 gives it. A moved test keeps its name. Where a spec cites the file of a moved test, the step changes the citation in this change's delta spec.

## 1. The baseline and the rule

- [ ] 1.1 Measure a full gate on deneb before any step. Take its wall time from sluice's records. Sample the count of `sluice` and `git` processes each 10 seconds, and keep the peak. Write the numbers in design.md's Measurements.
- [x] 1.2 Propose the rule `logic-is-tested-as-units-and-few-tests-run-the-binary` in Boom, for the whole system. Check: `boom show logic-is-tested-as-units-and-few-tests-run-the-binary` gives it, proposed.
- [ ] 1.3 Put the end-to-end set of design D2 in a nextest test group `e2e` with `max-threads = 4`, in `.config/nextest.toml`. Check: `sluice exec main -- moon run cli:tools`.

## 2. The pull request driver (171 tests)

- [ ] 2.1 `Pulls` takes the remote's address, a head's fetch and a branch's push through `RequestRefs` (design D3). `PushedRefs` implements it, and `sluice_fakes::World` implements it. Check: `sluice exec main -- moon run cli:test`.
- [ ] 2.2 Move each scenario of `crates/sluice/tests/profiles/pull_requests.rs` to an in-process test over the fakes, `FileForge` and the fakes' `RequestRefs`, but `submit_opens_then_updates_a_pull_request` and `landed_and_merged_by_landing`. Move each scenario of the forge's HTTP to a unit of `rest.rs`. Check: `sluice exec main -- moon run cli:test cli:tools`.

## 3. The host process (120 tests)

- [ ] 3.1 Run the host process's handler in-process, on tokio's paused clock, with a client over an in-memory stream (design D4). Move each test of `crates/sluice/tests/host_process.rs` that proves a decision to it. Check: `sluice exec main -- moon run cli:test cli:tools`.

## 4. worktrunk and the outcomes (77 tests)

- [ ] 4.1 Move each test of `crates/sluice/tests/profiles/worktrunk.rs` but the four of design D2. An outcome's delivery, `sluice inbox`, `sluice confirm` and a wait's following move to tests over the fakes, beside `cli.rs`. Check: `sluice exec main -- moon run cli:test cli:tools`.

## 5. The forge (63 tests)

- [ ] 5.1 Move each test of `crates/sluice/tests/profiles/forge.rs` but the three of design D2 to units of `app.rs`, `rest.rs`, `keyless.rs` and `workflows.rs`. Check: `sluice exec main -- moon run cli:test cli:tools`.

## 6. The git profiles (122 tests)

- [ ] 6.1 Move the logic of `pushed_refs.rs`, `remote_refs.rs` and `local_refs.rs` to pure functions with units: the router's reading of each copy's policy, a refusal's words, a lease's expectation. Keep one contract test of each profile (design D2). Check: `sluice exec main -- moon run cli:test cli:tools`.

## 7. The checks, places and moon (153 tests)

- [ ] 7.1 Move the logic of `checks.rs`, `profiles/places.rs` and `profiles/moon.rs` to units: the run's environment, the ports' blocks, the scratch's path, moon's plan from its query's answer. Keep the contract tests of design D2. Check: `sluice exec main -- moon run cli:test cli:tools`, and `cli:sandbox` outside the sandbox.

## 8. The other files (189 tests)

- [ ] 8.1 Move `exec.rs`, `remote_branch.rs`, `protect.rs`, `peers.rs`, `overview_watch.rs`, `previews_tools.rs`, `instance.rs`, `telemetry_logs.rs`, `runs.rs`, `equip_tools.rs`, `stack.rs`, `landed.rs`, and the five tests of `cli.rs` that start the binary. Remove `profiles/conformance.rs` (design D2). Check: `sluice exec main -- moon run cli:test cli:tools conformance:test`.

## 9. The duplicates in-process

- [ ] 9.1 In `cli.rs`, the conformance crate and the units, find each pair of tests that prove one behaviour. Remove one test of each pair. Check: `sluice exec main -- moon run cli:test conformance:test core:test`.

## 10. The end

- [ ] 10.1 The `tools` task runs the end-to-end set alone, and no other test starts the binary. Check: `grep -l CARGO_BIN_EXE_sluice crates/sluice/tests` names only the files of design D2.
- [ ] 10.2 Measure as in 1.1, and write the after row.
