# Hand-off: U5, the plugins hold one subscription each

As of 2026-11-23. The change `the-plugins-hold-one-subscription-each` is built on the branch `change/the-plugins-hold-one-subscription-each`. Tasks 1.1 to 4.2 are done. Task 4.3, the gate's verdict, waits on the gate.

## The gate's first run

`sluice exec main` at 316c2359a, on 2026-11-23: 15 tasks passed, and `cli:tools` failed on one test.

- `sluice::pull_requests run_again_marks_a_draft_ready` failed after 108 s. Its waiting `wt submit` exited 4, where the test expects 1 (`crates/sluice/tests/profiles/pull_requests.rs:3031`). The wait said `#2 withdrawn by resubmission: #3, which carried its work, failed`.
- The same test, run alone at the same head with `sluice exec main -- cargo nextest run -p sluice --features tools --test pull_requests -E 'test(run_again_marks_a_draft_ready)'`, passed twice, at about 34 s each.
- The change does not touch the pull request driver. The failure came under the load of the whole tools tier.
- sluice recorded no result for `cli:tools`: its sandbox refused moon's writes of task outputs under `sluice.git/moon-outputs/`, and sluice read the run as one that could not run.

## What `sluice overview --json --watch --stdin` does that this change does not fix

Seen in a live herdr session with the plugin of this branch and a sluice built from it:

- Where no `sluice host` runs, the client starts `sluice host` as its own child. The process tree shows `sluice host` under `sluice overview --json --watch --stdin`.
- The client leaves one finished child unreaped: a `<defunct>` process under it for as long as it runs.

Both belong to `sluice-host-answers-every-read` (D7), not to the plugin.

## Herdr tests that the sandbox skips

Under `sluice exec`, macOS's sandbox refuses `ps`, so these 4 tests of `plugins/herdr/tests/run.sh` skip, under sh and under dash:

- `watch: a watcher stops only its own sources`
- `watch: a killed watcher's sources are ended`
- `watch: a source is known without ps's start times`
- `watch: a killed watcher's source ends`

On Linux, where `/proc` can be read, they run. The last one was checked by hand in the live herdr session: after `kill -9` of the watcher, the next pane event started a new watcher, which ended the old source and its reader and ran one source for all three repositories.
