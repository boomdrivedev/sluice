## Context

The statement's clause 154 has each entry's life be a span of its own,
from its submission to its end, landed, refused, dropped for a
conflict, blamed or withdrawn, naming how it ended and holding its time
waiting with no candidate of it under test and its time under test; a
run that ended with no outcome is named so and is never the entry's
failure (114). sluice's metrics add, by branch and how an entry ended,
each entry's time waiting and under test, the entries ended, and the
host's running checks against its bound with how long a run sluice
chose waited for room under it (71). No metric names an entry's head,
its submitter or the context it was submitted under. Rule core.53, as
amended at 8e18722f (on `design/a-sluice-command-joins-its-callers-trace`),
rules out a run with no outcome named as its entry's failure and a
metric with an entry's head, its submitter or a trace context as an
attribute. Rule core.66 (the measurements change) has every metric
carry its repository, its branch and, where it has one, its outcome,
and nothing else, and every time declared in seconds.

This change builds on two changes of the same bolt, which land first
(proposal, "Depends on"): the trace change gives a submission its kept
`trace`, the binary's kept tracer `TRACER`, `telemetry::traceparent`,
and the hook `sluice_core::links`; the measurements change gives
`Peer.repository`, `METRIC_KEYS`, `SECONDS` and the meter's view that
declares each `SECONDS` names in `s`.

What sluice has today:

- **A submission's record** (`records.rs`, `Submission`) holds its
  `submitted` time, its `state` and its submitter (`from`: `agent`,
  `worktree`, `host`), and no time of its testing. A combination's
  record holds `opened`, when it was chosen for testing, and its runs'
  ends; combinations go once their submissions land or leave.
- **Under test.** `derive_states` (`step.rs`) reads a submission as
  under test while a combination on the branch's revision (`doc.base`)
  in `CandidateState::Testing` holds it. Its `State::Testing` is not
  the same: a submission whose own combination failed reads `testing`,
  and one whose own combination passed reads `passed` while a deeper
  one holding it is still under test (117).
- **Its ends** are written at nine sites: a landing (`perform_landing`),
  a landing the forge refused, work found landed (122), work landed
  beside the chain (140), a blame, a conflict, a withdrawal, a
  resubmission and a request closed. A submission refused by the forge
  goes back to the queue where another of its landing leaves
  (`drop_left_landing`), and is pushed again on the operator's word
  (152).
- **Every write** of a branch's records goes through `Sluice::carry`
  (`lib.rs`), reached by `Sluice::write` and `Sluice::update`, which
  writes `line.changes()` with the version the line was read at. A
  write the records refused for a newer version is read again and made
  again. A home's carry (`home.rs`) writes through the home store, not
  `carry`. `carry` already clones the line to set `doc.kept` for the
  lead, and reads the clock only where it needs it.
- **A run's end** is written at three sites: the round's own run
  (`poll`), a run no live host took at its time limit (`untaken`), and
  `end_run`, a lapsed claim's or a run at the bound's. `Ended` holds
  each check's outcome, `passed`, `failed`, `unproven` or `none`, and
  `timed_out`, `why` and `untaken`.
- **Metrics** are `tracing` events in the core, read by the binary's
  `MetricsLayer`; the core depends only on `tracing` (record 0014).
- **The host's room** (`host_process/room.rs`; rule core.49) counts
  every run against the bound through the host's registry
  (`host_count.rs`), whose entries name the repository and the place
  and no branch. A combination's wait is kept per repository
  (`queue_waits`), and decides the share (record 0123); a session's or
  a preview's per asker (`session_waits`), in order. The check seam's
  `hold(kind)` names no branch; a session's or a preview's ask reaches
  the host process as `Request::Hold { kind, repository, place }`. The
  host process looks every 100 ms and scans every three seconds
  (`serve`).

## Goals / Non-Goals

**Goals:**

- A submission shows as one span from its submission to its end, in the
  trace it was submitted under, saying how it ended and how long it
  waited and was under test (154).
- A run with no outcome is named on that span and never makes it a
  failure (114).
- The metrics give each submission's waiting and testing, the
  submissions ended, the host's running checks against its bound, and
  each run's wait for room, by repository, branch and, for an end, its
  outcome, and nothing that grows with submissions (154; core.66).

**Non-Goals:**

- Changing the metrics a landing already sends.
- Showing a submission's time under test in any output.
- A pool's runs, which take no part of a host's bound.

## Decisions

### D1. A submission's end is the write that moves it into an end state

`State::ends()` holds for `landed`, `refused`, `failed`, `conflict` and
`withdrawn`. `Line` keeps each submission's state as read
(`read_states`, filled by `Line::load`), and `Line::ended()` gives each
submission in an end state that was read in none, or not read at all.
After `carry`'s write is written, each submission `ended` gives is sent
(D4, D5); a write the records refused sends nothing, and the write made
again sends it once, on whichever host or process makes it.

An end is named by its state's own word, the one a status shows:
`failed` for 154's blamed, and `conflict` for its dropped for a
conflict.

A submission the forge refused that goes back to the queue, or is
pushed again on the operator's word, ends again later; that end sends a
second life, from its submission to the second end. A home's carry
writes through the home store and ends nothing. A submission written in
an end state by the write that makes it, as a submit of work the branch
already holds (core.48), ends there, having waited nothing.

Not taken: sending the end at each of the nine sites that write one,
where one missed sends nothing; and reading the branch's log at the
end, which is kept in parts a round does not read (core.41).

### D2. Every write keeps a submission's time under test on its record

`Submission` gains `under_test_ms: u64`, the milliseconds a combination
holding it has been under test, through the last write, and
`under_test_since: Option<String>`, RFC 3339, since when one is (each
`#[serde(default, skip_serializing_if = ...)]`). A submission is under
test while it is in no end state and a combination on the branch's
revision in a state `CandidateState::under_test` reads holds it, as
`derive_states` reads it. Combinations that overlap, a shallow and a
deeper one both under test, count once.

`Line::under_test_moved()` says, reading no clock, whether any
submission's record disagrees with that reading: under test with no
`under_test_since`, or not with one. `Line::keep_under_test(now)` sets
`under_test_since` where one began and, where one stopped, adds
`now - under_test_since` to `under_test_ms` and clears it. `carry`
keeps it on every write before reading the line's changes: where the
time moved, it clones the line, as it does to set `kept`, and asks the
clock once. A submission's end stops its time in the write that ends
it, since a submission in an end state is under test no more.

The fields are a fact of the submission's record, kept whether or not
telemetry is on, on every host that writes the branch; nothing sluice
decides reads them, and no output shows them yet.

Not taken: summing each combination's time from `opened` to its
verdict, which counts overlapping combinations twice and loses those
removed at a landing; and keeping it in the process that tests, which
another host or a restart loses (core.11, core.14).

### D3. A run that decided nothing is counted on its submissions

`Ended::decided_nothing()` holds for a run its time limit did not end
in which no check passed or failed: one no host took (`untaken`), one
whose every check is `none` or `unproven`, and one given no check.
`Line::count_no_outcome(candidate, &ended)` adds one to
`runs_with_no_outcome: u32` on each submission the combination holds
where the end decided nothing. Each of the three sites that write a
run's end calls it with the end it writes.

The life names the count as `sluice.no_outcome` (D5). A run with no
outcome never makes a submission's life a failure: its outcome is the
end state the core wrote, and the core blames only on a verdict that
failed (114; core.35). Telemetry reads the end; it judges nothing.

Not taken: a span event for each such run, which needs a time the
record does not keep.

### D4. The core hands a life to a hook, and sends its metrics as events

`sluice_core::life`, beside `links`, holds the hook: `Life`, the facts
a life is sent with, `set(fn(&Life))`, which the binary calls once, and
`ended(&Life)`, which does nothing until it is set. A `Life` holds the
repository (`Peer.repository`), the branch, the number, the head, the
work (`from.worktree`), the agent (`from.agent`), the kept `trace`, the
`submitted` and `ended` times, the outcome's word, the seconds waiting
and under test, and the runs with no outcome.

For each submission a written write ended, `carry` gives the hook its
`Life` and emits, as `tracing` events, `monotonic_counter.sluice.submissions.ended`,
`histogram.sluice.submission.waited` and
`histogram.sluice.submission.tested`, each with `sluice.repository`,
`sluice.branch` and `sluice.outcome`, and nothing else (core.66). Its
time under test is `under_test_ms` in seconds; its time waiting is the
seconds from `submitted` to the write's time less that, never below
zero. The write's time is the clock `carry` asked for D2, or asked once
where nothing kept a time and something ended. `SECONDS` gains
`sluice.submission.waited` and `sluice.submission.tested`.

Not taken: an event the binary turns into a span in a layer of its own,
which the logs layer would also send as a log record (record 0022: only
a failure's output and sluice's warnings are logs).

### D5. The binary sends a life as one span with its own times

While traces go, `Telemetry::begin` sets the life hook, as it sets the
link hook. It builds `sluice.submission` with the tracer it keeps
(`TRACER`), its start the submission's `submitted` and its end the
write's time, by one function, `life_span(tracer, &life, ending)`,
that a unit test calls with a tracer of its own:

- its parent is the kept `trace`, read as `telemetry::traceparent`
  reads it; where none was kept or it does not read, the span roots a
  trace of its own, never nesting under the span current where the end
  was written;
- it links to the span current where the end was written, the round's
  or the command's, where that span is sent, so a person goes from the
  life to the act that ended it;
- it holds `sluice.repository`, `sluice.branch`, `sluice.submission`,
  `sluice.head`, `sluice.work`, `sluice.agent`, `sluice.outcome`,
  `sluice.waited_seconds`, `sluice.tested_seconds` and
  `sluice.no_outcome`;
- its status is an error for `failed` alone, so nothing but a blame
  reads as the submission's failure.

Not taken: the life under the trace of the round that ended it, which
is one of many rounds and no submission's own; and a span opened at the
submit and closed at the end, which no process holds across hosts and
hours.

### D6. A run's part of the bound is held for its branch

`Tests::hold(kind, branch)` names the branch the run is for, which each
of the round's calls gives as its line's branch. The check profile, the
host's room wrapper, the fakes, the conformance suite's recording
checks and `sluice-core`'s test host pass it on. In the room,
`Shared::hold(kind, repository, branch)`, `Room::ask(asker, repository,
branch, now)` and `Room::wait(repository, branch)` take it; a held part
keeps it, and the registry's `Entry` names it as `branch:
Option<String>` (`#[serde(default)]`). `Request::Hold` carries `branch:
Option<String>` (`#[serde(default, skip_serializing_if = ...)]`), which
`Client::hold` and the host-process seam's `hold` send: `sluice exec`'s
the branch its run is held to, a preview's its branch, and a check
profile beside the host process its round's.

`Room::running_by_branch()` gives the runs the room counts against the
bound by repository and branch: the registry's live entries, or the
room's own count, and the parts it holds. An ask or an entry naming no
branch, an older sluice's, counts toward the bound as before and in no
branch's row.

Not taken: reading the branch from a place's name, which is the place
profile's to choose (core.2); and counting a branch's runs from its
records, which hold no session's run.

### D7. The room times each run's wait for room, by branch

Beside `queue_waits`, which decides the share and which nothing here
changes, the room keeps `waited_since`, keyed by repository and branch:
a combination's wait from its first refused ask, renewed by each ask
and lapsing as `QUEUE_WAIT` has a repository's. A session's or a
preview's wait is timed from `Room::wait`, which records `since` on its
`SessionWait`. Each part held records its repository, branch and the
time it waited, zero where its first ask was held. `Room::waits()`
drains them.

Not taken: keying `queue_waits` by branch, which changes the share and
the count of waiting combinations the sessions' turn reads (150).

### D8. The host process sends the room at each scan

At each scan, every three seconds, `serve` drains the room's waits and,
while its telemetry sends metrics, emits for each repository and branch
with a run counted then or at the last scan
`gauge.sluice.host.running`, its runs counted, and
`gauge.sluice.host.bound`, the host's bound, a row whose runs ended
reading 0 once; and for each wait drained
`histogram.sluice.run.waited_for_room`. Each carries `sluice.repository`,
named once for each repository as its binding names it
(`Bound::peer().repository`), and `sluice.branch` (core.66). One
function, `measure_room`, does it, taking the room's figures and the
names, so a test drives it under a subscriber of its own. Waits are
drained whether or not anything is sent, so they never gather.
`SECONDS` gains `sluice.run.waited_for_room`.

The bound is the same on each row: a person reads `sluice.host.running`
summed over branches against `sluice.host.bound`'s greatest.

Not taken:

- *The bound with no repository or branch*, which core.66 rules out.
- *The round sending its branch's runs*, which no round runs for a
  session's run on a branch with nothing queued, and which the host
  process's count holds already.
- *An up-down counter of runs*, which a host process started afresh
  begins at zero while the runs of the one before it still go.

### D9. What names a submission goes on its span alone

The head, the work, the agent and the kept trace are the life span's
(D5); no metric carries them (154; rules core.53, core.66). The round
tests' helper that holds every metric to `METRIC_KEYS` holds the new
ones too.

## Risks / Trade-offs

- **A submission the forge refused that ends again sends two lives**
  (D1), counted twice in `sluice.submissions.ended`, once `refused` and
  once as it ended. Rare, and each is an end that happened.
- **An older sluice that rewrites a submission's live record drops its
  time under test**, which then counts from the next write that finds
  it under test. Telemetry decides nothing, and the hosts of one branch
  move to one version.
- **The time under test moves at writes,** which a round makes; a
  combination judged between rounds counts until the round that reads
  its verdict, as `sluice.candidate.tested_for` does.
- **An older sluice's run counts in no branch's row,** so the rows sum
  short of the bound's count while one goes.
- **A host process started afresh** has no last scan to zero a row
  from; the row the one before it sent stops where it stood, until its
  branch runs again.
- **A combination's part given back unstarted and held again** is
  timed twice, the second wait from its next refusal.
