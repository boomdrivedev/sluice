# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A test that a signal stops stops its host processes

- [x] 1.1 In `crates/sluice/tests/support/guard.rs`, add `hold`, `let_go` and the stop on `SIGINT`, `SIGTERM` and `SIGHUP`. In `crates/sluice/tests/support/mod.rs`, make `Scratch` hold its folder while it lives (D1). Check: interrupt `cargo nextest run -p sluice --features tools --test peers -E 'test(a_signed_record_is_verified_once)'` after four seconds. No host process of the test runs one second after. The task `cli:lint` passes: `sluice exec main -- moon run root:lint`.

## 2. The leak script fails a task that leaves a host process

- [x] 2.1 In `tests/leaks.sh`, fail the task where a host process of the test runs one second after the test ends (D2). Send a stop signal that the script gets to the test's group. Read a pid as a host only while its command line is the host's program with `host` or `daemon`. Add the four cases to `tests/leaks-cases.sh`. Check: `sluice exec main -- moon run root:leaks-cases`.

## 3. The crate's test tasks run under the leak script

- [x] 3.1 In `crates/sluice/moon.yml`, run `test`, `tools`, `sandbox` and `speed` under `tests/leaks.sh`, with it as an input (D3). Check: `sluice exec main -- moon run cli:test`.
- [x] 3.2 The shell tasks that run under `tests/leaks.sh` pass. Check: `sluice exec main -- moon run cli:e2e plugin-sluice:test-hooks plugin-sluice:test-lay-out plugin-herdr:test-sh plugin-herdr:test-dash`.

## 4. The change

- [x] 4.1 Run `openspec validate a-test-stops-the-hosts-it-starts --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 4.2 Archive the change with `openspec archive a-test-stops-the-hosts-it-starts`.
