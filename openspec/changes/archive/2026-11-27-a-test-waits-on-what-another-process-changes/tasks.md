# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. The tests wait on what another process changes

- [x] 1.1 Add `Host::reads` in `crates/sluice/tests/peers.rs`, and read the first host with it in the two tests. Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test peers`.
- [x] 1.2 Read the running sources one time for each look in `watch: source exits`, in `plugins/herdr/tests/run.sh`. Check: `sluice exec main -- moon run plugin-herdr:test-sh plugin-herdr:test-dash root:format`.

## 2. The change

- [x] 2.1 Run `openspec validate a-test-waits-on-what-another-process-changes --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive a-test-waits-on-what-another-process-changes`.
