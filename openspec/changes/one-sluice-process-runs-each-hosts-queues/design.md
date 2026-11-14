# Design

## Context

Clause 135 is this change's subject, with 129's reader, 71's count and
150's turns, and 137's intake. Record 0093 holds the decision, with its
amendments of 2026-11-08 (a queue executes whole, in one of four ways)
and 2026-11-13 (every read follows the host process; a result is read
once). Records 0123 (a freed run goes to the repository with the fewest
under test), 0016 as amended 2026-11-13 (a place holds its ports) and
0107 (a process kept running takes up changed settings) ride on the same
process. Rules core.25, core.31, core.33, core.49, core.10 and core.54
bind it. The family's principles 4 to 6 say why: a tool that needs a
background process starts it and stops it when idle, and each shared
thing is decided in one process.

This was built once, on `bolt/sluice-runs-one-host-process` (e61acba)
and its fix branch (47ef4b8a), as the changes
`a-hosts-queues-move-in-sluices-own-host-process` and
`requests-are-taken-in-with-no-intake-mark` (record 0103 there, never
on main). Main has since moved more than five hundred commits: the
settings take-up and `sluice run`'s hand-over (record 0107), weir's
deliveries (record 0102), numbers from a branch's own records (record
0114), a run whose checkout was not made (record 0118), pools (record
0119) and more. The old bolt is the reference for what was built and
what its gate found; this change rebuilds it on main and adds what was
decided since. Where a decision below is the old bolt's, it says so,
and the build ports that code rather than writing it again.

What main holds at 7ce112b4 that this change replaces or moves:

| What | Where |
|---|---|
| a repository's driver | `Command::Run`, `drive`, `Driving` in `crates/sluice/src/lib.rs`; `keep_running` in `main.rs`; `readers/<repository>.driver` in `reader.rs` |
| a wait that runs rounds | `wait_on` and `Reading` in `lib.rs`, rounds unless `reader::driver_live` |
| the reader | `reader.rs`: lock, document, version, lease under `$XDG_STATE_HOME/sluice/readers/` |
| the host's count | `host_count.rs`: `running/`, an entry per admitted run; `checks::Checks::admit` at `Tests::start`, so the round's count (`step.rs` `capacity`, `choose_starts`, `starting_here`) and the admission are two steps |
| sessions waiting for room | `exec.rs` and `preview.rs` loop on `session_capacity` and `AtBound` |
| one-shot reads | `status` and `overview` read through `State::read`, which fetches a remote-held branch, and `overview::document` asks every remote (`refresh`); `Sluice::results` parses the whole results log, every signed record verified per process |
| ports | `places.rs`: `hold_block`, `free_block`, `ports/<n>`, `Ports::first(n)` reckoned from the policy wherever a place's ports are read |
| settings | `host_settings.rs`, `instance.rs` (`changed_tables`, `AFRESH`), `hand_over.rs` (`sluice run`'s `execve` with a driver's descriptor and the supervisors) |
| intake | `pulls.rs` (`Pulls::takes`, `decide`, `put_mark`, `hand_over`), `pushed_refs.rs` (`intake_mark` and kin), `kept.rs` (`intake/`), `lib.rs` (`Taker`, `Holder`, `TakenInBy`, `crate::intake` and its `thread_local`) |

Commands are bound to one repository each (`main.rs`,
`profiles::Bound::open`). `Bound` holds `Rc`s, so one `Bound` lives in
one thread. The tests over the fakes (`tests/cli.rs`) run `sluice::run`
in their own process against `support::FakeHost`; the tests on disk run
the binary with `SLUICE_ROUND_SLEEP=record`, which skips every pause.

## Goals / Non-Goals

**Goals:**

- One host process for each user on a host, started by a command when
  work arrives and gone when idle, nothing installed (135).
- Every repository's queues moved in it, each apart (26), found again
  after a restart from an index the commands write (135).
- The host's bound decided in it alone: a run's part held from its
  count to its start, turns between combinations and sessions, sessions'
  and previews' runs in the order they came, and a freed part shared
  among repositories by who has fewest under test (71, 150; core.49;
  record 0123).
- Every read of a queue, a watch, a wait, a status, an overview, the
  home view and a forecast, following it and fetching nothing of its own
  while it has read within its tick; results read once, by key, and
  never taken away with a branch (129, 69, 109).
- A place's ports its own, fixed when it is made (31, 32).
- Changed settings taken up between passes, and a change that cannot be
  taken up in place taken by a fresh host process it starts (135;
  core.54).
- No intake mark and no hand-over anywhere: the instance that keeps a
  branch on its remote takes its requests in, each request keeping one
  number (137, 120).
- Every test that drives rounds by `sluice step`, a wait or `sluice run
  --rounds` keeps driving them, on the fakes and on disk.

**Non-Goals:**

- A queue's home written in its records, one lead while a run is live,
  and a machine whose signature a branch does not accept leading none of
  its rounds (record 0122; the next change).
- A run's life written before it acts; GitHub Actions running a
  queue's checks; the forge's workflow named in the refusal (record
  0122, 0119; later changes).
- `sluice daemon`; a preview asked for with no wait keeping its place.
- One forge client shared across repositories.
- Whether `sluice submit` waits by default.

## Decisions

### D1. One host process for each state folder, and where it is found

The old bolt's D1, as built. `sluice host` is a hidden command, as
`sluice supervise` is. It takes `$XDG_STATE_HOME/sluice/host/lock` for
its whole life and exits at once where another holds it, so there is one
process for each user on the host (135). Beside the lock:

```text
$XDG_STATE_HOME/sluice/host/
  lock           held by the host process while it lives
  start.lock     held by a command starting it, and by the process as
                 it decides to end or hands over (D6, D20)
  version        its version, pid and start time, written whole
  log            what its passes print, its warnings and failures; the
                 previous process's kept as log.1
  repositories   the index (D4), written whole
<the user's directory>/host-<digest>.sock
```

The socket is in the user's own short directory, `/tmp/sluice-<uid>/`
as record 0063 makes and checks it, or the directory
`SLUICE_USER_SCRATCH` names, named by a digest of the state folder's
real path, so each state folder has its own process and a deep state
folder, a test's or a sandbox's, never puts the socket past macOS's 104
bytes (the core.30 incident of 2026-11-03). Record 0093 says "a socket in
sluice's state folder"; its build amendment says where and why. Not
taken: the socket under `host/`, refused where too long, which fails
every test whose scratch is deep.

The process binds the socket only while it holds `lock`, removing a
socket file a dead process left first. The lock goes with its process
however it ends, so a crash leaves nothing to wait out. A state folder
removed, or made again at its path, ends the process at its next tick,
and it writes nothing there again (47ef4b8a's `folder_gone`).

### D2. Reaching it and starting it

`host_process::client::reach(repository)` connects and says `hello`
with its version; the process answers its version, pid and start time.
Requests and answers are lines of JSON, one request for each
connection, except a follow or a held part, which keep theirs. Where
nothing answers, the command takes `start.lock`, tries once more, and
otherwise spawns `current_exe() host` detached: a new session
(`setsid`), working folder `/`, stdin null, output to `host/log`, the
environment of D3. It polls the socket for up to ten seconds and lets
`start.lock` go. Ten commands at once take `start.lock` in turn: the
first starts the process and the nine find it answering. A process that
cannot be started within the ten seconds is an error naming `host/log`,
never a silent wait.

The requests:

- `serve {repository}`: the repository has work. The process adds it to
  the index (D4), starts its task where none runs, and runs a pass at
  once.
- `step {repository, branch}`: one round of the branch (D10).
- `follow {repository, kind}`: keep the repository read while the
  connection is open; kind is a watch, a wait or `sluice run`. A pass is
  asked at once where none is under way, and each pass's report is
  written down the connection.
- `pass {repository}`: run a pass now and answer once it ends (D14).
- `read {repository}`: answer once a reading of the repository is
  current (D16).
- `results {repository, branch, ask}`: the results that match `ask`
  (D17).
- `hold {kind, repository, place}` and `started {run}`: a session's or a
  preview's part (D12).
- `room`: the bound, what counts against it, the runs waiting, whose
  turn is next and each repository's runs under test.
- `hand-over`: end, for a newer sluice (D8).

### D3. The environment it starts with

The old bolt's D3. The host process starts with the host's own
environment, never the starting command's place (135): what
`checks::KEEP` keeps for a run and `LC_*`, with `XDG_*`, `SSH_AUTH_SOCK`,
`OTEL_*`, `DOCKER_*` and the `SLUICE_*` settings, `SLUICE_AGENT` left
out, and of git's own the variables that set where its configuration
comes from and whether it may prompt (`GIT_CONFIG_GLOBAL`,
`GIT_CONFIG_NOSYSTEM`, `GIT_TERMINAL_PROMPT`, `GCM_INTERACTIVE`). It
keeps no variable that names a repository, worktree, index or object
store, no multiplexer session's, and no development shell's. The list is
one function, `host_process::environment::kept`; the check profile names
the shell variables it leaves out, so no tool's name is in the core
(core.1). `PATH` is the starter's, as every run's already is; a run's
equipping enters its policy's toolchain first (147; core.46). Not taken:
the login shell's `PATH`, a shell's start at each start.

### D4. The index, and the look at an idle repository

The old bolt's D4 and its gate's fix 8.8. `host/repositories` is a JSON
list of common git directories written whole under `host/start.lock`.
A command adds its repository once it has read a policy at a revision
the host holds or found records there ("Index, on any sluice use", the
user's choice). One that finds its repository listed writes nothing.

The host process reads the index at its start and whenever a request
names a repository it does not hold. At its start and every three
seconds it looks at each listed repository it runs no task for, as an
idle tick reads it (129): each remote that keeps its branches' records
asked once, so work another host wrote there is found, then the records
and the previews, with no other process started
(`host_process::index::in_play`). Each look runs on a thread of its own
under the round's retry policy (134), never on the loop that accepts
connections, and a repository still being looked at is passed by; a
spent allowance is passed by, never waited out (core.29). A look that
finds work in play starts the repository's task. A listed repository
whose directory is gone, or which has no sluice, is taken out of the
index.

### D5. A task for each repository

The old bolt's D5. Each repository with work in play or a follower has
a thread of its own, which opens its own `Bound` with the round's retry
policy. The pass `drive` runs today moves into
`host_process::pass::pass`: the remote's one ask, the releases, the
intake, a round of every branch in play, the intake again of a branch
whose round stepped, and the reader's document; it returns a report of
what it printed and the branches it passed by. The intake after the
round posts what the round decided on each pull request, a request sent
back marked a draft, before the pass writes the document a follower
reads, as the wait's own intake did once a round had run; it asks the
forge nothing where the round left the branch and its records as they
were (136). Not taken: the round before the intake, which takes a
request's submission in a pass late. A refused policy or a slow fetch holds only its
own thread (26). A panic is caught, logged with the repository, and the
task started again at the next tick. A task ends once its repository has
nothing in play and nothing follows it. It ticks every second while a
watch follows, three seconds otherwise (`sluice::tick`).
`check_host_systems` runs at the process's start; a host whose
configuration names a system it is not claims no run and says so in its
log.

### D6. When it ends

The old bolt's D6. After each tick, the process ends where no task runs,
no look is under way, no connection is open and no part is held or
asked for. It takes `start.lock`, accepts every connection already
queued without blocking, and ends only where none came; then it removes
the socket, lets `lock` go and exits. A command that met no answer
meanwhile waited on `start.lock`, and finds nothing answering once it
has it, so it starts a new process; none is lost between the two. Work
in play is a submission queued or under test, a checkout to release or a
preview to drop (47ef4b8a's `previews_in_play`).

It keeps nothing the records and the host's state do not hold. Runs are
supervised in their own process groups and outlive it (core.10). A
process killed mid-round leaves what a stopped `sluice run` leaves, and
the next process's rounds carry it on from the records (S10; 122).

### D7. The reader is the host process

The old bolt's D7, with the reading's marks of D16. For each repository
it holds, the task takes the reader's lock and writes the document as
the driver does today, and takes `readers/<repository>.driver` so an
older sluice's wait or watch finds a driver live and follows. Where an
older `sluice run` holds that lock, the task runs no round of that
repository while it lives and follows its document; the other
repositories go on. An older reader holding the reader's lock gives way
by the lease, as a newer watch takes it today.

The host process marks a reading's start as each pass of a repository
begins and its end as the pass ends, so a reading under way spans its
rounds and a status during a long landing is answered from the last
reading rather than fetching (D16). A watch or a wait opens a `follow`,
which keeps the task alive and is told each pass. A follower whose
connection drops reaches the process again at its next tick, starting it
where none answers (129), reading the last document until the new
process writes one.

### D8. A newer sluice takes over

The old bolt's D8 and its gate's fix 8.9. `hello` answers the process's
version. A command whose version is newer, compared as `reader::older`
compares, sends `hand-over`. The process stops its ticks; each task
finishes its round, lets its locks go and ends, given a minute
(`tasks::FINISH_FOR`); the process ends as in D6 without waiting for
idleness. The newer command waits for the older process's lock to go,
for that minute and a start's ten seconds after
(`client::HAND_OVER_WAIT`), then starts its own; an older process still
running then is an error naming `host/log`. An older command meeting a
newer process uses it. A request the process does not know is answered
with an error naming both versions. A build from a checkout is 0.0.0,
older than any release, so a test's binary never displaces a host's
installed sluice.

### D9. Waits and watches follow, and run no round

The old bolt's D9. `wait_on` loses its round and its intake. It opens a
`follow`, whose first answer comes once the pass it asked for has
written its document, so a wait's first pass, whatever its timeout, is
the host process's (main's "A timeout returns the submission as it
stands"); then at each tick it reads as a follower. `Reading::driven`
and `driven_by` go. A round's failure on the wait's branch, read from
the document, is said once on stderr and the wait goes on. A wait
reports a landing once its record says landed or its branch holds its
work (core.48); what a round failed after the landing is in the host
process's log and the branch's state, never the wait's exit, and exit 3
is only for a read the wait itself could not make. A wait asks GitHub
only for a pull request it follows, so that is the one read on which it
meets a spent allowance and waits it out within its limit (134); a wait
on a submission meets none, the host process's pass passing the forge's
part by. A submit writes its
entry as today, sends `serve`, and returns with `--no-wait` or waits as
`sluice wait` does; `wt submit` is a waiting submit. `sluice overview
--json --watch` follows as a wait does.

### D10. `sluice step` is the host process's round

The old bolt's D10, with record 0103's D6. `Command::Step` sends `step`;
the repository's task runs, between its own passes, the remote's one ask,
the branch's intake, `s.round` and the intake again where the round
stepped, as a pass does, so a step reads each signer's runs as a pass
reads them (D17), and answers the document today's `step --json` prints,
`outcome`, `released`, `previews_released`, `releases` and `held`, with
`warnings`, each warning the round's intake met. The command prints the
document under `--json` and otherwise the lines today's step prints,
and says each warning on stderr as `sluice: warning: taking in
<branch>'s pull requests: <asked>: <said>`, every step. The exits are as
before. A step's round is the host process's round, so `sluice step` and
a pass behave alike (core.14).

### D11. `sluice run` follows

The old bolt's D11. `sluice run` stays a command, hidden from the help,
so a host's unit that runs it keeps working. It checks the host's
systems, says once on stderr "sluice starts its own host process;
nothing needs `sluice run`", and opens a `follow` of kind run. Each
pass's report comes down the connection and it prints the lines a pass
of `sluice run` printed; `--rounds n` ends after n passes, exiting as the
last did. A follower holds nothing its settings decide but its own
telemetry, so its take-up (D19) is the look it keeps today and, for a
change that moves `[telemetry]`, its `execve` start afresh, its
document carrying the passes followed toward `--rounds` and what it
said; the driver's descriptor and the supervisors leave the document,
since a follower holds neither (`hand_over.rs`). Not taken: removing the
command, which turns each host's unit into a restart loop until retired;
dropping its take-up, which leaves a follower sending to a collector the
file no longer names (135).

### D12. The host's room in one place

The old bolt's D12 and its gate's fixes 8.1 and 8.7.

**The seam.** `Tests` gains `hold(&self, kind: RunKind, repository) ->
Result<Option<Part>>`, and `start` takes the part. `Part` is opaque to
the core, and dropping one gives its room back (record 0087, option 1).
`choose_starts` holds a part for each run it would start, runs opened
again first, then the window's, before it writes the decision that marks
the run starting; where `hold` gives none it marks no further run
starting. The round keeps its parts through its landings, releases and
outcomes and hands each to its run's `start_run`. A run marked starting
with no part, a stopped round's, is read against the host before it is
opened again: where the host cannot meet what it needs it is recorded as
a run whose tests could not run, and the checkout its stopped start made
is released either way. A start the check seam refuses at the bound
opens again each run decided behind it, its part dropped and no place
made (71). A run a lead marks starting under the claim it gives a pool's
job is no stopped round's: the job's round holds its part as it starts
it, as `start_claimed` does. `capacity`, `starting_here` and
`perform_starts`'s second reading of `running()` go.

**The room.** `host_process::room::Room` is one value in the host
process behind a mutex. It counts the registry's live entries, the
parts it holds, and the waiting runs with their kinds, repositories and
times. Its admission is record 0087's, with D13's share:

- A combination's part is held where a run of the bound is free, unless
  that run is one a session may take, a session's or a preview's run
  waits and the turn is the sessions'; and only for the repository D13
  puts first. Held in room a waiting session could have taken, the turn
  becomes the sessions'.
- A session's or a preview's part is held where a run of the bound less
  `[runs] reserved` is free and owed to no one: no session's or
  preview's run waits ahead of it, and either the turn is the sessions'
  or the combinations waiting are fewer than the free runs. Held, the
  turn becomes the queue's.
- A refusal records or renews the run's wait, and a hold removes it. A
  combination's wait counts within thirty seconds of its round's last
  ask; a session's or a preview's while its connection is open.

**The registry.** The room admits through `HostCount` under
`running/.lock`, so an older sluice's `sluice exec` on the host still
counts and is counted; each part writes its entry with a hold the room
keeps locked until the run's supervisor holds the run directory, and a
part dropped unstarted unlocks at once. A room made afresh counts the
entries whose supervisors still hold their runs.

**Sessions and previews.** `sluice exec` and `sluice preview` send
`hold` and keep the connection while they wait; the process answers
`waiting` with why, "the host is at its bound" or "a combination's run
starts first", each said once, and `held` once the room is theirs. The
command starts its run with the part and its `started` line ends the
connection's hold. Ctrl-C closes the connection and gives back the part
or the wait. The check profile's `Tests` in a command process holds
through this connection, and in the host process through `Room` itself.

**What a person reads.** The overview's `room` gains `waiting`, by
kind, `turn`, `queue` or `sessions`, and `repositories`, each one's
runs under test and runs waiting. Its `host_process` is `{version, pid,
since}`, or null where none answers, beside `host`, the host's name.
`sluice status` with no target prints the same room line.

### D13. A freed part goes to the repository with the fewest runs under test

Record 0123, built in the room. A repository's runs under test are its
runs holding a part of the bound, of any kind: a combination's, a
session's and a preview's. Whenever a combination's part is asked, the
room puts first, among the repositories with a combination waiting
(asked within thirty seconds) and the one asking, the repository with
the fewest runs under test, and among equals the one whose oldest
waiting combination has waited longest; a repository that asks with no
wait recorded waits from that ask. A part is held only for the
repository first; any other is refused and its wait recorded. So a
repository with a run ready and none under test takes the next part
that frees, ahead of every repository holding one. Within a repository,
which of its runs takes the part is the round's own order (13). Nothing
under test is stopped. A run launched on a pool holds no part and counts
in no repository's share (record 0119). A run waiting for its share reads
as waiting for room since when (66, 87).

When a part frees, a run ending or a part dropped, the room tells the
task of the repository first to pass now, as `serve` does, so the
repository it is owed to takes it within a moment rather than at its
next tick, and a repository whose turn comes before another's ask is
not passed over by a busier one asking first. Under passes when asked
(D14) nothing passes by itself, so the tests' own steps decide.

Not taken: counting a repository's combinations alone, which lets one
whose agents hold the bound with `sluice exec` take candidates' parts as
if idle; a fixed share per repository in the host's settings, and a run
stopped to make room (record 0123's options 2 and 3).

### D14. Passes when asked: the tests' host

The old bolt's D13. Under `SLUICE_ROUND_SLEEP=record`, the value the
tests already set to skip pauses, the host process takes no pass of its
own: it runs a pass only for a `pass` or a `step` and answers each once
the pass ends; a `serve` holds the repository for the next ask, a
`follow` asks a pass at each of its follower's ticks, a `read` answers
at once from the last reading, and a freed part wakes no task. A test's
rounds are those its commands asked for, in its own order. The process
still checks each real second whether it should end, so a test's
scratch removed ends it.

`exec::Host` gains `host_process(&self) -> &dyn HostProcess`, the
requests of D2 as methods. `Bound` answers through the socket.
`support::FakeHost` answers in the test's process: `pass` and `step`
run the pass or the round on the fakes' seams, `serve` holds the
repository for the next ask, `hold` asks a `Room` it keeps, `read`
answers at once, `results` reads the fakes' log, and every follower asks
a pass. The tests over the fakes keep their commands, and their rounds
run through the same `host_process::pass`. Not taken: a setting that
keeps the host process from starting, with waits running rounds as
before, a behaviour 135 removes kept behind a flag (core.12).

### D15. Which commands reach it

- **`serve`, starting it where none answers:** a submit, waiting or
  not, by worktree, by name or by request; `withdraw`, `amend`,
  `set-aside`, `branch create`, `branch accept`, `branch delete`, and a
  `release` of a
  checkout or preview.
- **`follow`:** `sluice wait`, a waiting submit, `wt submit`, `sluice
  overview --watch` and `sluice run`.
- **`step`:** `sluice step`.
- **`hold`:** `sluice exec` and `sluice preview`.
- **`read`, starting it where none answers:** `sluice status`, a
  one-shot `sluice overview`, the home view and a forecast (D16).
- **`results`:** any command whose core reads results, `sluice exec`'s
  session run and `sluice set-aside` among them (D17).
- **Never:** `hook`, `credential`, `supervise`, `protect`, `forge`,
  `inbox` and `equip` beyond the index. They read no queue.

### D16. Every one-shot read follows the host process

Record 0093's amendment of 2026-11-13. A one-shot read, `sluice status`
in each form, `sluice overview` without `--watch`, the home view and a
forecast, sends `read {repository}`, starting the host process where
none answers. The process answers once a reading of that repository is
current: one that ended within its tick, or one under way that began
within a tick of the last one's end (the forecast's rule of 64 today),
and in either case one that began after the last `serve` a command sent
for the repository (D15), so a read after a submit, a withdrawal or a
branch made or removed reads what changed.
Where it holds no current reading, because no task runs there or the
task is idle between ticks, it reads now, by the look of D4 or the
task's next pass, and answers when that reading ends; its answer names
the reading's end and, where the reading could not ask a remote, the
remote's words. The command waits for the answer within the round's
retry bound and a tick; past that it reads as the host holds the
repository and says on stderr when the host process last read it.

The command then reads with nothing fetched:

- **The overview and the home view** print from the reader's document
  of that reading, the text rendered from it as the watch's document is
  (`BranchView` is read back from it today, `branch_in`); where no
  reader keeps its tick, or the look made the reading, which writes no
  document, from the records as the host holds them. The `read` answer
  says when the look made the reading (`looked`), so a document an
  earlier pass wrote is never printed beside records the look read
  since.
- **A status**, in each form, reads with the core in held mode: every
  branch through `State::read_held`, as the host holds it since the host
  process's last ask, no `refresh`, the pull requests it follows as the
  host last kept them (`Follows::Kept`), the room from `room`, and
  results by key from `results` (D17). The host process's reading asks
  GitHub for each followed request when it falls due, at the host's one
  bound of once in a quarter of `discovery` (record 0095; core.33), so
  what a status and the overview show of one is as fresh as that bound
  allows.
- **A forecast** forms from the reader's document as it does today
  beside a live reader, and otherwise on every branch as the host holds
  it. Where a revision it forms from is not on the host, it sends `read`
  once more with a reading asked afresh: a branch of which that reading,
  current and refused by no remote, brings no revision is no branch,
  refused as a forecast refuses one, exit 2; what is still missing
  otherwise is said as a read it could not make, exit 3.

A read thereby fetches nothing of its own while the host process keeps
its tick, and loads no results but those it asks for. Not taken:
running the read inside the host process, the command's code over the
task's binding, since a task mid-pass would hold a read behind a landing
for minutes and the read's warnings would go to the host process's log;
a read that fetches for itself where the reading is stale, which is
today's cost.

### D17. A result is read once and answered by key

Record 0093's amendment of 2026-11-13, and 69 and 109.

**The seam.** `State::results(branch)` becomes `State::results_for(
branch, ask: &ResultAsk) -> Result<Vec<u8>>`. A `ResultAsk` names keys
(a check's task and hash), combinations and tasks, and a line matching
any is answered. Every caller asks for what it reads: a candidate's
verdict its checks' keys and its combination, a session run its plan's
keys, a verdict view its submission's combination, a landing's second
reading of its verdict the combinations it lands, `set-aside` the task
it names. None asks for the whole log, and `every_result` goes.

**The index.** `host_process::results::Index`, one for each repository
in the host process, `Send + Sync` behind a mutex, shared by the
repository's task and the connection threads that answer `results`. It
reads the local results log from the byte offset it last read, the whole
log again where the file shrank or was replaced, and each signer's ref
as the tick last fetched it, by entry: a blob not yet seen is verified
once with the key the asking branch's policy lists, kept by blob,
signer and key digest as `RemoteRefs::verified` keys it today. Lines
are kept by key, combination and task. In the host process, the records
profile answers `results_for` from the index; in a command process, it
sends `results`; the fakes answer from their own log. The index lives
for the host process's life, so no read loads or verifies the whole
history again, however long it grows.

**Nothing takes a result away.** No landing, branch removal or release
removes a result or a signed record (69, 109; C.6). A test holds it: a
pass recorded on a bolt that is removed still counts for main's gate.

Not taken: pruning a landed branch's results, which takes away passes
other branches' gates count and evidence a landing names (record 0093's
amendment); an index in a file each command reads, a second store of
results to keep in step.

### D18. A place holds its ports

Record 0016 as amended 2026-11-13. A live place's ports are held on
the host by the port: `$XDG_STATE_HOME/sluice/ports/held/<port>`, each
naming the record of the place that holds it, under `ports/.lock` as
blocks are today. A new place takes, from `ports_from`, the lowest run
of as many ports as its policy names in which no port is held by a live
place, one whose record exists, and each port is free. Its record is
written first, so a hold names a record that exists, then each port is
held, then the record is written with `ports`, each name and its port.
Every reading of the place's ports afterwards, its runs' `SLUICE_PORT_*`,
its release, a preview's names and a status, reads that record and never
reckons them from a policy; a release frees each port its record names.
`hold_block`, `free_block`, `Made::block` and `Ports::first` go. A
session's place, which holds its ports from its first run, takes them
the same way.

An older sluice's block files, `ports/<n>`, are read by nothing; the
check that each port is free still catches a stack listening on one
(Risks). Not taken: one block size for the whole host, which bounds
every repository's port list (the record's other path); reading older
blocks through their places' policies, a map from one shape to the
other (principle 12).

### D19. The host process takes up changed settings

Record 0107 as amended 2026-11-13. The host process holds the host's
settings with their sign (`host_settings.rs`), and its main loop looks
between ticks, never within a pass: the file's sign alone, no process
started and no git read (core.31). A change it can take up in place is
bound afresh and the new binding's host systems checked; then each
repository's task binds afresh at the start of its next pass, so no pass
runs under two settings (core.14), and the room's bound and reserve
change from its next admission. Parts held and runs started keep what
they were given. It says in its log `sluice: took up the host's settings
in <path>: <tables> changed`. A file gone, unreadable or refused, or
settings that do not bind, leave it on what it holds, said once in its
log by the sign kept. Each task's first pass under a new binding reads
each policy once more, as a process started afresh would (record 0107's
decision 7).

### D20. A change it cannot take up in place goes to a fresh host process

Record 0107 as amended 2026-11-13; boomdrive-design's word of that day:
the hand-over kept is the host process giving its repositories to a
successor on the same host, as it gives them to a newer sluice's (D8).
Where the tables a change moves include one of `instance::AFRESH`,
`[telemetry]` alone today:

1. It binds the new settings and checks the host's systems; where
   either fails it keeps everything and says once, by the sign kept,
   `sluice: warning: <path> changed <tables>, which the host process
   takes up only by handing over, and its settings do not bind, so it
   keeps the settings it holds until the file changes: <why>`.
2. It stops its ticks; each task finishes its pass and lets its locks
   go, within `tasks::FINISH_FOR`; it sends what its telemetry holds.
3. It takes `start.lock`, lets `lock` go and removes its socket, and
   starts `<the executable it started as> host`, resolved through every
   link as it started, with the environment it was started with,
   detached as D2 starts one.
4. It waits up to ten seconds for a `hello` from a process with another
   pid, then lets `start.lock` go and exits. A command that met no
   answer meanwhile waited on `start.lock` and reaches the successor.
5. Where no successor answers, it takes `lock` back, binds its socket
   again, keeps the settings, binding and telemetry it holds, resumes
   its ticks, lets `start.lock` go and says once `sluice: warning:
   <path> changed <tables>, which the host process takes up only by
   handing over, and its successor did not start, so it keeps the
   settings it holds until the file changes: see <log>`.

Nothing passes between the two but what the records and the host's
state hold: the successor reads the settings file itself, finds its
repositories in the index and their work in the records, and counts the
runs still supervised (D12). Runs already started keep the settings
they started under; they are supervised apart and outlive the process
that started them, their supervisors taken up by the system when it
ends. Work in play never waits for another command to start a host
process. Not taken: `execve`, as a kept `sluice run` does, which keeps
a pid nothing follows and carries a document of what it held, where
nothing need pass; a hand-over to whichever process the next command
starts (the plan's first words), which leaves work in play waiting.

### D21. The instance that keeps the branch takes its requests in

Record 0103's D1. `Pulls::takes(s, t, branch)` answers `Here` where the
branch's records are on its remote and `Sluice::keeps(branch)` is true,
and otherwise `Elsewhere` with the host the branch's records name as its
lead, or none where none leads yet. Any process of that host takes them
in under intake's lock, a pass, a step and a submit's own first intake
alike, so the records' one lead and intake's one lock make one taker
(72; core.14). `Taker` goes; `takes` reads no mark and never answers
`Undecided`. A host that does not keep the branch asks GitHub nothing of
its open requests, withdraws nothing, and follows its own requests by
GitHub's word and sluice's check on their heads (137; records 0095,
0096). Not taken: every host taking in a branch whose records each host
keeps, each request taken in once per host (137); the first host that
finds no lead in a file or ref of its own, a mark by another name
(core.33).

### D22. A pull request table on records each host keeps refuses its branch alone

Record 0103's D2. `Rules` gains `unkept`, set where the table has
`pull_requests` and the policy names no `[signers]`. `Policy::rules_for`
refuses that branch alone with `NoIntake { branch, remote }`: `main
takes pull requests, which only the host that keeps it on origin takes
in, and its records are each host's own: name [signers] so they are kept
on origin (137)`. Every other branch reads as before (56, S64). Routing
reads the branch's remote by `Policy::remote_of` whatever its table
refuses (core.27). A submit to it exits 2 before anything is pushed or
opened; a step, a pass and the look pass it by with its words, every
other branch moving; `status` and the overview show it;
`PullRequests::branches` passes it by. The refusal names the forge's
workflow once a change builds one a command writes; naming `sluice init
actions` today would send its reader to a command that does not exist
(core.26). It stands as a requirement of its own in `pull-request-intake`,
so `actions-pool`'s block of "A branch turns pull requests on in its
policy" still holds every scenario once this change is archived. Not
taken: the whole policy refused (S64); the table accepted and its
requests left waiting, a request opened that nothing takes in (137); the
table's intake alone refused, half a branch kept.

### D23. Nothing of the mark is kept, and nothing reads an older one

Record 0103's D3. `PushedRefs` loses the mark's refspec and the
`--prune` it alone needed, `intake_ref`, `intake_mark`,
`intake_mark_at`, `put_intake_mark`, `drop_intake_mark`, `leased_ref`,
`at_the_lease`, `Mark` and its `Holder`; `Kept` loses `intake/` and its
`.waits/`. A mark an older sluice left on a remote is fetched, read and
deleted by nothing; `refs/sluice-fetched/<remote>/intake/*` and
`forge/intake/` on a host are read by nothing. The pull requests guide
says how to delete a remote's mark by hand, `git push <remote>
:refs/sluice/intake/<branch>`. Not taken: a sweep pushing to a ref sluice
no longer owns, which an older sluice on another host still reads.

### D24. No hand-over is written, and an older one is read as written

Record 0103's D4. `Requester::HandOver`, `Pulls::hand_over` and
`Sluice::drop_obsolete`, whose one caller it was, go; a round still drops
the combinations every other withdrawal makes obsolete. `Withdrawal::
handed_over_to` and `Event::Withdrawn`'s stay in `sluice/submission@1`
and `sluice/log@1`, read and never written; an older hand-over holds no
request (record 0088 as amended; core.48), so a branch whose records
move onto its remote takes each such request in again at its head with
no draft. With no withdrawal for a hand-over, a request taken in keeps
one submission and one number from intake to landing, however many
hosts follow it. Not taken: an older hand-over read as any withdrawal,
holding its request as a draft for an act no submitter made (120).

### D25. A host that does not keep the branch shows the requests it follows

Record 0103's D5. Record 0095's rows become every host's that does not
keep a branch whose records are on its remote: `opening` and
`follow_across` keep what a submit, a wait or a status follows under
`forge/follows/`; `PullRequests::across` and `kept_across` read every
such branch's follows where this host does not keep it, each row a
request whose submission the branch's queue does not hold at its head.
`taken_in_by` is `{"host": <lead>}`, `null` where none leads; `holder`
goes. The host process's reading reads each followed request that falls
due (D16); a status, the overview and the reader's document carry the
rows as the host last kept them. The rows' heading reads `pull requests <host> takes in:`, a wait's
words `waiting to be taken in by <host>`. `kept_across` reads the rows
again once the kept file or the branch's lead moved (129; core.25,
core.31). Not taken: no rows, which 137's last sentence rules out;
`taken_in_by` dropped, which crew reads.

### D26. What a step and a submit say of intake

Record 0103's D6. `PullRequests::intake` returns its causes beside
whether the branch takes pull requests: what it asked, `<branch>'s pull
requests` or `pull/<n>`, and the words it was refused with. Nothing in
`pulls.rs`'s intake path prints. `pass::step` puts each in the answer's
`warnings` (D10); a submit's own first intake says each on its stderr; a
pass hands them to D27. The spent allowance's line is no such warning: it
stays said once for its pause (134). Not taken: a step reading the host
process's log for its round's lines, which another round may write
between.

### D27. A standing intake failure is kept, logged and said as a refused read is

Record 0103's D7. Intake's failures are reads of the forge the core
could not make (core.28), so they take 133's one design:

- the records seam keeps, beside a branch's refusal of its remote, this
  host's refusal of its intake: the same `Refusal` document under
  `intake/<branch>.json` where the first is `refused/<branch>.json`, in
  the local files, the remote refs and the fakes;
- `Sluice::note_intake_refused(branch, causes)` keeps it and writes a
  `read-refused` line, `{"host","remote","asked","said","intake":true}`,
  for each cause not yet logged, in a write of its own or the next write
  of the branch's log (`Sluice::carry`); `Sluice::note_intake_read`, at
  an intake that met no failure, ends it with a `read-again` line so
  marked; `intake` is skipped where false, so the round's own lines are
  written as before;
- a pass says a cause in the host process's log only when its note
  logged it, read from the kept document's `logged`, never a
  `thread_local`, so a restarted host process, a step and a submit say
  nothing again (core.11, core.14);
- `sluice status <branch>` shows it above the queue, `this host could
  not take in <branch>'s pull requests since <t>, <n> rounds: <asked>:
  <said>`, and `--json` carries it as `intake_refused`; nothing a round,
  gate, landing or submission decides reads it.

Not taken: the round's own refusal document, which every round that read
the branch would end; events of their own, which an older sluice fails
on reading a branch's log on its remote; core.47's count at every
refusal, a write per pass.

### D28. The suite, and the changes built on the mark

Record 0103's D8. `OnGitHub` in `tests/profiles/pull_requests.rs` gives
`main` `[signers.a]` with a key made for each fixture, `trust = ["a"]`,
and the host's `[signing]` before `[runs]`, so main's records are on the
bare remote and host `a` keeps it; `other(name)` gives the policy that
host's signer where a test asks, and a test across hosts lets `b`'s pass
run first, so `b` keeps `main`. The tests of the mark, its lease, holds,
renewal and hand-over are taken out where nothing of them stands, or
rewritten to the host that keeps the branch. `pulls_setup` in
`tests/cli.rs` names a signer. `openspec/changes/a-host-hands-over-only-
to-the-host-its-mark-names` and `a-hand-overs-withdrawal-holds-no-request`
are removed: their deltas describe the mark and were never archived, and
what of them stands is this change's `request-intake-host` delta.

## Looking across the family

- **flume.** Its C.6 step is nothing under record 0093; a settle reads
  each place's outcome through `sluice status` or `wait`, which starts
  the host process where none runs (D15, D16). flume's rooms (70, 130,
  145, 181) and sluice's (150) agree: a wait is the host's, equals start
  in the order they came, and the count and the choice are one step.
  The share among repositories (D13) is sluice's alone: flume bounds
  agents, not checks.
- **The herdr plugin.** Record 0094 keeps one `sluice overview --json
  --watch` per repository a pane is in; each is a follower (D7), so the
  host process lives while panes watch, as 129 has a reader live while
  followed.
- **crew and the board.** Their status and overview polls become reads
  that cost a socket round trip and a held read (D16), the cause of the
  2026-11-13 signal. crew reads `taken_in_by` and loses `holder` (D25).
- **The runner.** Boomdrive's core.13 and record 0003 name sluice's
  driver among the services a host keeps; nothing of sluice's is kept
  now. That repository's change is its own (principle 11).
- **Settings.** flume, weir and the runner read their own settings;
  sluice's take-up between passes is record 0107's, and D19 and D20
  make the host process the one sluice process that does it with the
  queues (principle 5).
- **One decider in one process** (principle 6). The rounds, the bound,
  the share, the readers and the results index are each decided in the
  host process; the registry stays so an older sluice still counts and
  is counted, and so a process that starts counts the runs that outlived
  the last; the allowance's pause stays in the host's state.

## Risks / Trade-offs

- **One process holds every repository.** A fault that ends it stops
  every queue until the next command. Tasks run in threads of their own
  with panics caught (D5), any command restarts it (D15, D16), and a
  crash loses nothing (D6).
- **A read now costs a socket round trip, and may start a process.** A
  status in an idle repository starts the host process, which reads and
  ends. That is 129's cost, against today's fetch and 60 MB read.
- **The index holds every result in memory.** It holds parsed lines of
  each repository with a task or a `results` ask, for the process's
  life; a host whose history is very large pays memory once rather than
  a read per command, and gives it back when the process ends idle.
- **Followers keep it alive.** A watch per repository with open panes
  keeps the process up all day, idling as 129 has an idle reader idle.
- **Mixed versions during a rollout.** An older wait finds the driver
  lock held and follows; an older `sluice run` keeps its repository's
  rounds and the task follows it (D7); an older `sluice exec` admits
  through the registry the room counts (D12); an older reader gives way
  by the lease. An older sluice's place holds a block the new allocator
  does not read (D18): a session's place made before the release keeps
  its block until released, so the release notes say to release
  worktrees' places with `sluice release` before installing, and the
  check that each port is free catches a stack still listening.
- **sluice's own `main` and WilldanGroup/switchboard-kit's take pull
  requests and name no `[signers]`** (D22): each is refused on the
  release that carries this until its policy names signers whose keys
  its hosts hold. The release notes say so first, the orchestrator is
  told, and `sluices_own_policy_reads` finds sluice's own `main` refused
  until `.config/sluice.toml` names signers, which is ops' choice.
- **Naming `[signers]` keeps every branch with a `remote` on its remote**
  (C.6): a repository whose `main` names signers moves its bolts' records
  there too, each run signed by a key its policy lists.
- **Two copies of one repository on one host share its name among its
  peers, so both keep the branch**: each takes its requests in under its
  own copy's intake lock until the one-copy design decides (137;
  core.48's details).
- **The share wakes tasks.** A freed part passes the owed repository at
  once (D13); a host with many repositories waiting passes one more
  repository per freed part, which is the decision's point.

## Migration Plan

1. Before a host serving a branch that takes pull requests with no
   `[signers]` installs the release, that branch's policy names
   `[signers]` for the hosts that run its checks; the first host to route
   it there carries its records onto its remote.
2. Before installing, a host releases its worktrees' places held by the
   older sluice (`sluice release`), so no older block outlives it.
3. The release lands with the host process. A host's `sluice run` units
   keep running as followers until removed.
4. A remote's `refs/sluice/intake/<branch>` and a host's `forge/intake/`
   and `ports/<n>` files may be removed by hand.

Rollback is installing the previous release: its waits find no driver
lock once the host process has ended and run rounds as they did; it reads
`read-refused` and `read-again` lines with `intake` as 133's, the field
passed over; it reads a place record's `ports` as a key it does not
know, holding no block for it, so places made by this release are
released before rolling back.

## Open Questions

None.
