## Why

On 2026-11-26, sluice's gate failed five submissions on five different
tests. Each test passes when it runs alone. Each failure leaves a key
that the gate then reads as flaky. So each landing waits for the next
fix. We ran `cli:test` beside `cli:tools` many times on a busy host. We
listed each test that failed at least once. Four causes are in sluice,
and four are in the tests:

1. A host process that starts afresh in place lets the host's lock go
   before it replaces its image. The new image takes the lock again. Between the two, another process can get the lock. It can be
   a host process that starts. It can be a child that the old image
   forked, which holds a copy of the lock until it runs its program.
   Then the new image ends, or a daemon lets its socket go and waits. Failed:
   `a_newer_client_has_an_older_host_process_hand_over`, and
   `a_daemon_takes_up_a_changed_telemetry_table_in_place`, whose status
   got no answer during the start afresh.
2. A wait follows a pull request on what a submit read of it, for a
   quarter of the host's interval. When the request closed after that
   read, intake withdrew the submission of its new head. The wait then
   read the request open from the kept read, and ended naming the
   withdrawn submission, not the closed request. Failed:
   `a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes`.
3. A subscription sends each pass's report and each document as it
   comes, in no fixed order. When both are ready, the document can go
   first. A waiting submit that reads a landed document ends, and the
   report of the pass that landed it, with its failed release, is never
   said. Failed: `a_release_failing_after_the_landing_leaves_the_submit_landed`.
4. `sluice host` writes a hook through a temporary file named by its
   pid. A `sluice protect` and a subscription that installed the hook at
   one time wrote one temporary file. The first moved it into place,
   and the second could not find it. Two such writes can also both move
   the person's hook aside, so the second moves sluice's hook over the
   person's. Failed: `a_commit_during_a_long_round_ends_at_once`, on its
   `sluice protect`.
5. `sluice_run_with_nothing_queued_starts_git_at_its_first_pass_alone`
   counted a git that the watch's subscription started to find the
   hooks folder. The client sends a subscription with no wait for its
   answer. A run whose host passes only when asked ends on its pass. So
   the finding went on into the six passes that the test counts. The
   test did not wait for the watch's first document.
6. `an_upgrade_keeps_its_runs_as_children` read the host's log after
   the landing. Each host process that starts keeps only the log of the
   one before it. A host process ended when idle, and another started.
   The line of the hand-over was gone.
7. A status that failed during a test gave no reason: the test helper
   of `instance.rs` read the submissions of a status it did not check.

8. `a_waiting_submit_by_name_in_a_bare_clone_reports_another_queues_landing`
   read the submission's record as soon as the wait ended. The wait
   answers from the branch (rule core.48), before a round moves the
   record, and this test's host runs a round only when asked.

## What Changes

- A host process that starts afresh in place hands the host's lock to
  its new image by its descriptor. It hands the start lock and the
  socket so already. No other process gets the lock between the two images.
- A kept read of a followed pull request stands only while the
  request's entry in the host's records is as it was at that read. Once
  the entry moved, the follow reads the request again (rule core.32).
- A subscription sends a pass's report before a document that is ready
  at the same time. `sluice host` reads a document after the pass whose
  report it follows. So a client that reads a document has each report
  before it.
- `sluice host` writes a hooks folder for one request at a time.
- The tests of 5 to 8 read what they mean to read. The bare clone's
  test runs a round before it reads the record. The first waits
  for the watch's first document before the first run. The second reads
  the host's log at the hand-over. The third gives a status's output
  where it failed.

## Capabilities

### Modified Capabilities

- `host-process`: a hand-over in place hands the host's lock to the new
  image.
- `host-daemon`: a daemon's start afresh hands its locks to the new
  image, and keeps its lock where the start fails.
- `request-intake-host`: a kept read of a request stands only while its
  entry stands.
- `resident-subscriptions`: a pass's report comes before the document
  read after it.
- `branch-protection`: writes of one hooks folder at one time each end
  well.

## Impact

- `crates/sluice/src/host_process/mod.rs`: `start_afresh`, `run` and
  `daemon`, and `Carried`, which takes the lock's descriptor.
- `crates/sluice/src/profiles/github/pulls.rs`: `follow` and `holds`
  keep the entry beside each read.
- `crates/sluice/src/host_process/subscription.rs`: the loop's
  `select!` takes reports before documents.
- `crates/sluice/src/protect.rs`: `install_at`, `reinstall_at` and
  `remove` take one lock of the process.
- Tests: `host_process.rs`, `reader_tick.rs`, `instance.rs`,
  `profiles/pull_requests.rs`, and the tests of `subscription.rs` and
  `protect.rs`. Each
  race that sluice can meet has a test that forces it and fails without
  the change.
