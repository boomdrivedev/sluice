# Tasks

Each check runs through sluice: `sluice exec main -- <command>`. A herdr test is a `fresh '<name>'` case of `plugins/herdr/tests/run.sh`, which the tasks `plugin-herdr:test-sh` and `plugin-herdr:test-dash` run under `tests/leaks.sh`.

## 1. A subscription answers a forecast

- [x] 1.1 Add the ask `forecast`, the line `forecast` and `Serves::forecast` to `crates/sluice/src/host_process/subscription.rs`. Hold the forecasts in progress in a `FuturesUnordered` (D2). Check: `sluice exec main -- cargo nextest run -p sluice --lib host_process::subscription::tests::a_forecast_is_sent_once_it_is_made_and_holds_nothing_back`, on a paused clock.
- [x] 1.2 Answer it in `Subscribed::forecast` by the read of `sluice submit <branch> --check --json`, and take `forecast <branch> <worktree>` on the input of `sluice overview --json --watch --stdin` (D2). Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test overview_watch the_watch_prints_a_forecast_asked_on_its_input`.

## 2. The herdr plugin leaves no process

- [x] 2.1 Make `watch.sh --ensure` the watcher where none runs, and end it with the herdr server that started it. Ask another server to start the next by the action `watch`. Add `--register` for the popup (D1). Check: the herdr test `watch: a watcher ends with the herdr server that started it`, which fails before the change.
- [x] 2.2 Read a source's identity in the watcher at its next tick, and remove the subshell that did it (D3). Check: the herdr tests `watch: a watcher stopped as its source starts leaves no process` and `watch: a source's command is left no child of the watcher's`, which fail before the change.
- [x] 2.3 Ask each forecast on the source's input, and keep its answer in the reader (D2). Check: the herdr test `watch: a forecast is asked on the subscription`, which fails before the change, and the tests `sidebar: *` of forecasts.
- [x] 2.4 Run each test's `--ensure` as herdr runs it, with no wait for its end. Check: `sluice exec main -- moon run plugin-herdr:test-sh plugin-herdr:test-dash plugin-herdr:manifest`.

## 3. Documents and the change

- [x] 3.1 Update `plugins/herdr/README.md`: the watcher's life and the forecast on the subscription. A person reads the diff.
- [x] 3.2 Run `openspec validate sluices-herdr-plugin-leaves-no-process-and-forecasts-from-its-subscription --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
