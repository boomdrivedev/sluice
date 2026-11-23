# Tasks

## 1. The runtime and the stops

- [x] 1.1 Build a tokio runtime with many threads in `sluice host` and `sluice daemon` alone, with a bounded pool for blocking work. Add tokio's runtime, net, signal, sync and time features, tokio-util and tokio-stream. Check: `cargo test -p sluice --test host_process an_idle_host_process_wakes_only_for_its_timers`.
- [x] 1.2 Hold every task in a `TaskTracker` under a `CancellationToken` tree, and give `SIGTERM` and `SIGINT` to every host process. Remove `stopping` and `crate::stopped` from the host process. Check: `cargo test -p sluice --test host_process sigterm_stops_a_host_process_after_its_round`.

## 2. The socket

- [x] 2.1 Serve the socket on `tokio::net::UnixListener` with `LinesCodec`, with the same requests and answers. Remove the 10 ms poll. Check: `cargo test -p sluice --test host_process a_connection_is_answered_with_no_poll`, and `sluice exec main -- cargo test -p sluice --test host_process` for the whole protocol.
- [x] 2.2 Carry the socket across a daemon's start afresh in place. Check: `cargo test -p sluice --test host_process a_daemon_started_afresh_answers_on_its_socket`.

## 3. Readings

- [x] 3.1 Give each repository a task with an inbox and a `watch` channel. A trigger during a reading makes one more reading. Check: `cargo test -p sluice --test host_process five_serves_during_a_reading_make_one_more`.
- [x] 3.2 Watch refs, `packed-refs`, the worktrees' `HEAD` and the records folder with notify-debouncer-full, and start a reading on an event. Check: `cargo test -p sluice --test host_process a_records_write_reaches_a_watch_by_its_file_event` and `a_burst_of_ref_moves_makes_one_reading`.
- [x] 3.3 End the three-second look. Read an idle repository on an event, a command, a follower's opening, or the minute's backstop. Check: `cargo test -p sluice --test host_process three_idle_repositories_are_read_once_a_minute`, and `sluice exec main -- cargo test -p sluice --features tools --test reader_tick`.

## 4. The clock

- [x] 4.1 Put each timer of the host process on tokio's clock, and have the core's `Clock` seam in the host process read it. Check: `cargo test -p sluice --test host_process the_backstop_falls_due_at_the_minute_on_a_paused_clock`.
- [x] 4.2 Run the host process's suites that slept for real on a paused clock where they can. Check: `sluice exec main -- moon run cli:test`, then `cli:tools`, one at a time.

## 5. Records

- [x] 5.1 Amend the record of the host process with this change's decisions, each with Before and Now in plain words. Check: `sluice exec main -- moon run root:changes`.
