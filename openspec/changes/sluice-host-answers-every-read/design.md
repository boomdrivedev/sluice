# Design

## Context

After `sluice-host-runs-on-a-tokio-edge` and `sluice-host-owns-its-workers`, `sluice host` runs on a tokio edge: its socket, connections, signals and timers are tokio tasks, which a `TaskTracker` holds under a tree of `CancellationToken`s. Each repository has a scheduler, a tokio task with one reading in flight at a time and a `watch` channel of its readings. Each look is a call of the synchronous core through `spawn_blocking`, and each round runs on the repository's worker, a thread that holds the repository's binding. The connections read lines with `LinesCodec`. Its workers are its children. This design cites the decisions of `sluice-host-runs-on-a-tokio-edge` as the edge's D1 to D6.

The reads are still made by the commands:

- A one-shot status, overview, home view and forecast send `read` and wait for a current reading (`lib.rs:run`, `host_process/ask.rs:read`). Then they bind the repository and read the records and the branches as the host holds them, in their own process (`overview.rs:document_held`, `home.rs:run`, `lib.rs:forecast_read`). Past the read's bound, the command reads the records itself and warns.
- A status gets results by `results` (`profiles/records.rs:results_for`). Where no host process answers, it reads the results log itself.
- A watch and a wait send `follow`, and read the reader's document from a file in the host's state (`reader.rs:Follow`). A wait reads its submission's record itself once the document no longer holds it (`lib.rs:Reading::read`). `sluice run` sends `follow` and prints each pass's report.
- `sluice host` holds a reader's lock, lease and version file and a driver's lock for each repository under `readers/`, so that a watch, a wait or a `sluice run` of a release from before it follows it.
- `sluice inbox` reads the deliveries in the records and records each hand-over in its own process.
- Each read command runs git in its own process to find its repository.

So ten statuses at one time make ten reads of one repository, and `sluice host` cannot give another tool a whole status or overview.

## Goals / Non-Goals

**Goals:**

- `sluice host` answers each read from the state that it keeps. A client sends its folder and prints what `sluice host` answers. (`reads-go-through-the-resident@69134e5825d7`)
- Asks of one read that come while it is in flight share it. (`one-read-in-flight@e99acda774d0`)
- Another tool gets each status and overview from the socket, and many folders on one connection. (`flume-has-a-resident@49876bc5c73c`, sluice's part)
- The protocol keeps only the requests that a command of this release sends.

**Non-Goals:**

- The changes, the git hook, `sluice previews`, `sluice exec` and its forecast. The bolt's unit `sluice-host-does-every-change` moves them. A read command still puts back the branch protection's hook as it does today, until that unit.
- A bound on the forge reads. `sluice host` asks the forge for a status as the command asks it today. The bolt's unit `sluice-host-is-the-only-github-client` bounds it.
- The plugins' own subscriptions. The bolt's unit `the-plugins-hold-one-subscription-each` changes them.

## Decisions

### D1. One request for each read, answered with its output

The protocol gets five requests beside today's: `status`, `overview`, `home`, `forecast` and `inbox`. Each names the folder and the command's own arguments, such as `target`, `all`, `branch` and `json` for a status. `sluice host` answers each with `output`: the bytes that the command prints on standard output, the lines that it prints on standard error, and its exit code. The client writes them and exits with the code.

```json
{"ask":"status","folder":"/w/sluice/feat","target":"main","json":true}
{"output":{"stdout":"{\"branch\":\"main\", ...}\n","stderr":[],"code":0}}
```

The rendering code of each command runs in `sluice host`, so each read prints what it printed before, in text and in JSON. A read that waits past its bound adds its warning to `stderr`, as the command wrote it before.

Not taken: the reading's data sent to the client, which renders it. The client then keeps a copy of the rendering code and of the records' types, and an older client would read a newer host's data. One renderer in `sluice host` gives one answer.

Not taken: one request that runs any command's arguments in `sluice host`. A typed request says which reads `sluice host` answers, and a change cannot come in as a read.

**A read names its caller's session.** Before: a read command took the agent's name and its pane from the environment of its own process. Now: each read also sends three variables of the caller's environment, `SLUICE_AGENT`, `HERDR_PANE_ID` and `HERDR_SOCKET_PATH`, as `caller`. The read's call in `sluice host` uses them in place of the host process's own environment. So `sluice inbox` hands an outcome over to the caller's pane, and names the caller's agent.

### D2. A client sends its folder and binds nothing

`main.rs` sends a read command to `sluice host` before it binds a repository. The client reaches `sluice host`, starting it on a bare install as today, sends its folder, and prints the answer. It runs no git, and opens no file of a repository or of the records. Its folder is its working folder, or `--worktree` where the command takes one.

`sluice host` resolves a folder with gix, in one call on the pool for blocking work: the repository's common folder, the worktree, its branch and its head. A folder in no repository gets the error that the command gave before, with its exit code. A folder that is no worktree, such as a bare repository, resolves as the command resolved it before.

The tests' host process, which answers in the test's own process, answers the new requests through the same `HostProcess` trait (`host_process/ask.rs`).

**A folder resolves with gix.** Before: a read command opened its folder with gix in its own process. Now: `sluice host` opens the folder with gix, on the pool for blocking work. A git worker gives other words for a folder in no repository, and gix gives the words that the command gave before.

**A read command reads the host's settings.** Before: a read command read the host's settings file as it started, and refused a field that it did not know. Now: the read command still reads that file and refuses such a field, before it asks `sluice host`. It reads no record and no result.

### D3. A read is one call on the blocking pool, beside the round

The connection's task answers a read. It is the task of the edge's D6, which the `TaskTracker` of the edge's D2 holds, under the connection's child `CancellationToken`. The task resolves the folder (D2) and asks `Tasks::read` whether the repository's reading is current (D4). The repository's scheduler, the tokio task of the edge's D3, decides only that. It does not run the read.

The read's core call is one `tokio::task::spawn_blocking` call on the bounded pool of the edge's D1. The call binds the repository as the host holds it. The binding fetches nothing and starts no round. The call reads the records and the branches, renders the command's output (D1), and drops the binding before it returns. A binding holds values that stay on one thread, so each call makes its own binding, as a one-shot command made its own. Asks that share a read (D5) share one call, so they share one binding.

The round runs in the worker's lane: the repository's worker is a thread that is entered into the runtime and holds the repository's binding while work is in play (`tasks.rs:Scheduler::bind`). The read's call never goes to the worker and never waits on the worker's inbox. A read waits only for a current reading, and only within its bound (D4):

- Where the last reading ended within a tick, the call starts at once.
- Where a step is in the worker's lane and no reading is current, the scheduler starts a look beside the step, on the blocking pool (`Scheduler::look_beside`). The look's end makes the reading current.
- Where a pass is the reading in flight, its end makes the reading current. Where the bound ends first, the call starts on the state that the host holds (D4).

A stop cancels the root token (the edge's D2). A read's call that runs is not cancelled. The tracker waits for it, as it waits for a look, within `FINISH_FOR`.

The read's git commands are workers of `sluice host`. Each starts through `crate::workers`, in a process group of its own, and is recorded before it works (`workers-end-with-their-owner@292e869158d6`). They take no slot of `[runs] max`, which counts checks only (`runs-max-counts-checks-only@4276e56de833`). Each git command runs on the pool thread of its call, and the call waits for it. So at one time, the read's git commands are no more than the threads of the blocking pool, `BLOCKING_THREADS` (256). The joined reads of D5 make one call for each key, so many asks make one set of git commands. (`workers-within-their-tools-limit@b48467b4ed47`, as `sluice-host-owns-its-workers` reads it with `runs-max-counts-checks-only@4276e56de833`)

A read of results asks the host process's index of results, as `results` does today. `sluice inbox` reads what is owed and records each hand-over, as the command did, and `--confirm` confirms in the same call.

Not taken: a thread of its own for each repository's reads, with a binding that it keeps. The edge runs each core call through `spawn_blocking` (the edge's D1), and a binding that a read keeps would hold state across reads.

**A read's binding uses the settings file as it is now.** Before: a read command bound the repository under the host's settings file as the command read it when it started. Now: the read's call binds the repository under the settings file as it is when the call starts. Where the file does not read, the call uses the settings that `sluice host` holds. A round takes up a change of the file only between two rounds, as before.

**A read's binding knows the branches that the last ask read as last fetched.** Before: only the binding of a pass knew which branches its ask could not fetch, and the reader's file carried that to a watch. Now: each reading gives those branches, with the remote's refusal, to the reads that it answers. So a status and a document say that a branch is as last fetched, in the remote's words.

### D4. A read's bound and tick are timers on tokio's clock

A read waits for a current reading within its bound, `read_for()`: the round's retry bound, `retry::COMMAND_FOR`, and a tick, `READ_TICK`. A reading is current where it ended within the tick. Both are timers on tokio's clock (the edge's D5): `Tasks::read` waits with `tokio::time::timeout_at` on the repository's `Marks` channel, and reads the tick from `tokio::time::Instant`.

Past the bound, the read's call runs on the records and the branches as `sluice host` last held them, and puts in `stderr` when it last read the repository. Where no `sluice host` answers, or it ends before it answers, the client exits 3 and says that sluice's host process did not answer. The client reads nothing itself.

A test of the bound builds the runtime with one thread and `start_paused`, and moves the clock with `tokio::time::advance`. A call on the blocking pool keeps the paused clock from moving by itself while it runs, so the test moves the clock past the bound while a fake reading holds the pass.

Before: past the bound, or with no host process, the command read the records and the results itself. Now: `sluice host` answers from the state that it last kept, or the command exits 3. (`reads-go-through-the-resident@69134e5825d7`)

### D5. Asks of one read share the read in flight, on a `watch` channel

`Joined<K, V>` joins the asks of one key to one call. It is a map from each key to one tokio `watch` channel. The channel's value is the newest answer and the generation of the call that made it. The map also holds, for each key, the generation of the call in flight, where one is in flight, and whether one more call is due after it.

An ask of a key:

- Where a call of the key is in flight and it began after a command last told of a change (`Tasks::told`), the ask needs that call's generation.
- Otherwise the ask needs the next generation. Where no call is in flight, the next call starts now. Where one is in flight, the next call starts when it ends. The asks that come meanwhile need the same next generation, so they make one more call, not many.

The ask takes a `watch::Receiver` of the key and waits with `Receiver::wait_for` until the value's generation is the one it needs, within the read's bound (D4).

The call is a task on the `TaskTracker` that awaits the `spawn_blocking` call of D3. When the call ends, the task sends its answer with `watch::Sender::send_replace`, with its generation. A `watch` channel holds one value, so the newest answer replaces the older one, and an ask that reads late reads the newest. Where one more call is due, it starts at once. A key with no receiver and no call in flight leaves the map.

Two maps use `Joined`:

- The folder's resolution (D2), keyed by the folder. The host process holds one map.
- The reads of D1, keyed by the request with its folder. Each repository has one map.

Each map is behind a `std::sync::Mutex`. No task holds the lock across an `.await`. The readings of the repository stay one in flight, by the `Marks` channel of the edge's D3. (`one-read-in-flight@e99acda774d0`, `tokio-streams-before-rxrust@0b9fc11cda8b`)

Not taken: futures-util's `Shared`. It joins asks to one future, but it keeps no newest answer after the future ends and has no generation for an ask that comes after a change. Not taken: tokio's `broadcast` channel. It keeps a queue of the older answers. Not taken: tokio's `OnceCell` for each call. It holds one answer and never replaces it.

**Each read's call is one line in the log.** Before: `sluice host` wrote no line for a read. Now: `sluice host` writes one line in its log for each read's call, with the request. The asks that join a call share its line. The tests count joined reads by these lines.

### D6. A subscription is a `StreamMap` of `WatchStream`s, keyed by folder

`subscribe` names the folders to serve. The connection stays open. The client sends `{"add":"<folder>"}` and `{"remove":"<folder>"}` lines on it. An `add` line can name how the client follows the folder, as `as`: `watch`, which is the default, `wait` or `run`. `sluice host` sends a `document` line for a folder when the folder is added, and after that each time the folder's overview changes in a part other than its time:

```json
{"add":"/w/sluice/feat","as":"wait"}
{"document":{"folder":"/w/sluice/feat","overview":{"schema":"sluice/overview@1","branches":[...],...}}}
```

The three ways keep what the kinds of today's `follow` did. While a watch follows a repository with work in play, `sluice host` reads it each second, and three seconds apart otherwise. A watch's or a wait's addition asks the remote at once. A folder added as `run` gets a `pass` line with each pass's report, in order, and no `document`. `sluice run` adds its folder as `run`, and prints each report as it prints a pass today:

```json
{"pass":{"folder":"/w/sluice/main","report":{"lines":["main: #4 landed as 9c1e2f0"],"failed":[]}}}
```

**Each repository's document.** Each repository's scheduler holds a second `watch` channel, of the repository's overview document. The overview is the reader's document, as `sluice overview --json` prints it. After each reading ends, where a follower holds the repository, the scheduler asks the overview read. That read is a read of D3, joined by D5 with each one-shot overview of the repository. The scheduler sends its answer with `watch::Sender::send_if_modified`, which sends only where the document changed in a part other than its time. So a receiver wakes only for a change.

**The connection.** The connection's task reads and writes lines with tokio-util's `LinesCodec` in both directions (the edge's D6): a `FramedRead` of the stream's read half for the client's lines, and a `FramedWrite` of its write half, which the task sends to with futures-util's `SinkExt::send`. After `subscribe`, the task holds:

- `documents`: tokio-stream's `StreamMap<PathBuf, WatchStream<…>>`, keyed by folder. Each stream wraps a receiver of the document channel of the folder's repository.
- `passes`: a `StreamMap<PathBuf, UnboundedReceiverStream<Report>>`, for the folders added as `run`. Each stream wraps the follower's channel of reports.
- `followers`: the `Follower` of each folder (`tasks.rs:Handle::follow`), of the kind that its `add` line names. A follower keeps its repository held, and counts toward whether `sluice host` is idle.

The task waits in one `tokio::select!` on four things: the next client line, `documents.next()`, `passes.next()` and the connection's child token's `cancelled()`.

- An `add` line resolves the folder (D2, joined by D5), takes the repository's `Handle`, takes a follower of the line's kind, and inserts `WatchStream::new` of the document channel under the folder. `WatchStream::new` gives the channel's current value first, so the folder gets its document at once. Where the repository has no document yet, the task passes over the empty value, and the first reading's document follows. An `add` of a folder that the map holds replaces its stream and its follower.
- A `remove` line removes the folder from both maps with `StreamMap::remove`, and drops its follower, which tells the scheduler (`Job::Unfollowed`).
- A document from `documents` goes out as a `document` line with its folder. A report from `passes` goes out as a `pass` line.
- The client's end, or a line that it cannot read, ends the task. The task drops each stream and each follower.
- The token's cancellation, at a stop or a hand-over (the edge's D2), ends the task at once.

**A slow client.** While the task waits for a write, it does not poll `documents`. A `watch` channel holds one value, so at the task's next poll each `WatchStream` gives the newest document of its folder, and never the older ones. Two folders of one repository hold two receivers of one channel, so they share its reading and its document. A `run` folder's reports come on an unbounded channel, in order, so `sluice run` gets each pass's report, as today.

A folder whose repository is gone, or has no sluice, gets the document that says so, and the task removes it. The connection ends when the client closes it or `sluice host` stops. (`flume-has-a-resident@49876bc5c73c`, `one-read-in-flight@e99acda774d0`, `tokio-streams-before-rxrust@0b9fc11cda8b`)

Not taken: one connection for each folder. A plugin that serves many repositories then holds many connections, which `one-subscription-per-plugin@dcc7b7fec9ee` rules out. Not taken: tokio-stream's `StreamExt::merge` of the streams. A merge cannot take a folder out, and `StreamMap` adds and removes by key.

**A folder added as `wait` gets each pass's report too.** Before: a follow of each kind got each pass's report. Now: a folder added as `run` gets each pass's report and no document. A folder added as `wait` gets each document and each pass's report. A folder added as `watch` gets each document alone. A wait uses the reports to say what each pass released, and a round's failure once.

**A gone repository stays gone.** Before: a watch found that its repository was gone by its own look. Now: `sluice host` sends the document that says that the repository is gone, whatever ends the repository's scheduler. The document has the words that its last read failed with, and `"gone": true`. No document read that is in flight replaces it.

**A stop ends the document reads.** Before: a stop waited for the reading of each scheduler. Now: a stop also ends each document read that is in flight, because no follower reads its document after the stop.

### D7. `sluice overview --json --watch --stdin` is a client with two threads and no runtime

`--stdin` needs `--watch`. The command opens one subscription with no folder. It reads its standard input: a line `add <folder>` adds the folder, and a line `remove <folder>` removes it. A relative folder is read from the command's working folder. It prints each document as one line, with `folder` in it. It ends when its input closes and no folder stays, or when it is stopped.

The client builds no tokio runtime (the edge's D1). It uses std's `UnixStream` and two threads of its own process:

- The main thread reads the socket's lines, and prints each document.
- The input thread reads standard input's lines. It writes each as an `add` or a `remove` line on a clone of the stream (`UnixStream::try_clone`), and keeps the set of folders.

At the end of its input, where no folder stays, the input thread marks that the input ended and shuts the stream down (`UnixStream::shutdown`). The main thread's read then ends, and the command exits 0. Where folders stay, the input thread ends, and the watch goes on. Where the main thread's read ends with no such mark, `sluice host` ended, and the command exits 3 as a read that `sluice host` did not answer. A signal ends the process with both of its threads. (`nothing-detached@f807c7f5fcea`)

`sluice overview --json --watch` with no `--stdin` is a subscription of its working folder, on the main thread alone. Its lines are as today, with no `folder` field.

Example, serving two folders on one connection:

```console
$ sluice overview --json --watch --stdin
add /w/sluice/feat
add /w/flume/main
{"folder":"/w/sluice/feat","schema":"sluice/overview@1","at":"2026-11-21T10:00:00Z","branches":[{"branch":"main","depth":0,"submissions":[],...}],...}
{"folder":"/w/flume/main","schema":"sluice/overview@1","at":"2026-11-21T10:00:00Z","branches":[{"branch":"main","depth":1,"submissions":[{"id":12,"state":"testing",...}],...}],...}
{"folder":"/w/flume/main","schema":"sluice/overview@1","at":"2026-11-21T10:04:12Z","branches":[{"branch":"main","depth":0,"submissions":[],"recent":[{"id":12,"state":"landed",...}],...}],...}
remove /w/sluice/feat
```

The two `add` lines go to `sluice host` on the one connection. The first two output lines are each folder's document at its addition. The third comes when flume's submission 12 lands. Nothing is printed for sluice's folder, since its overview did not change.

Not taken: a tokio runtime in the client, which the edge's D1 rules out, for a job that two blocking threads do. Not taken: `poll(2)` on the two descriptors, which needs nix's `poll` feature and more code for the same result.

### D8. A wait and `sluice run` follow a subscription on one thread

`sluice wait`, a waiting `sluice submit` and `wt submit` follow their submission with two things:

- A subscription of their folder as `wait`. From each document, the wait reads its submission, its combination's progress and its branch's hold, and prints them as it printed them before.
- The request `wait`, a step that `sluice host` answers. The step names the folder, the caller's session (D1) and the wait's arguments: its target, `--branch`, `--pull-request`, the waiting worktree, `--json` and `--confirm`. After the first step, it also names the submission that the wait waits on and the submission that it follows now.

```json
{"ask":"wait","folder":"/w/sluice/feat","target":"7","worktree":"/w/sluice/feat","own":{"id":7,"branch":"main"}}
{"waited":{"follow":{"own":{"id":7,...},"follows":{"id":7,...}}}}
```

`sluice host` takes each step from the records as it holds them. It answers with one of these:

- `ended`: the wait ends. The answer is the wait's report, as `output` (D1), with its exit code. `sluice host` records the hand-over of the outcome to the waiting worktree, and confirms it where `--confirm` asks.
- `follow`: the wait follows a submission, its own or a later submission that carries its work (record 0100), with what the step said.
- `pull-request`: the wait follows a pull request.

The wait asks a step at its start, where a document no longer holds the submission that it follows, where a document holds it decided or held, and where a round of its branch failed. Out of time or stopped, the wait prints its submission as the last document held it, and exits 5 or 130, as before.

**A wait reads no record.** Before: a wait read its submission's record itself, found what carried its work, and wrote its report and the hand-over in its own process. Now: `sluice host` does each of these in a step. `sluice wait` binds no repository, and runs no git. A waiting `sluice submit` and `wt submit` still bind the repository for the submit, which is a change.

**A wait that follows a pull request follows it as before.** Before: a wait that followed a pull request asked the forge from its own process. Now: the same. `sluice host` answers `pull-request`, and the command follows the request with its own binding. `sluice-host-is-the-only-github-client` moves the forge reads into `sluice host`.

**A group's wait reads its records as before.** Before: a `sluice submit` of several worktrees waited on each submission in its own process. Now: the same. The group is a change, and `sluice-host-does-every-change` moves it into `sluice host`.

The wait builds no tokio runtime. It reads the subscription's lines on one thread with std's `UnixStream`, with a read timeout. Where `sluice host` passes only when asked, the wait asks `pass` at each tick on a second connection. At its own timeout, it exits 5, as today.

**A `pass` names a folder or a repository.** Before: a `pass` named the repository's common folder, which a follower read from its own binding. Now: `sluice host` resolves the path that a `pass` names, as it resolves a read's folder (D2). So a client that binds nothing asks a pass of its folder.

`sluice run` adds its folder as `run`. It reads the `pass` lines on one thread with std's `UnixStream`, and prints each report.

**Each round of `sluice run` is a pass.** Before: each pass wrote the reader's file. The file event of that write started the next pass, so a repository with nothing in play was passed again and again, and `--rounds` counted those passes. Now: where the last pass found nothing in play, a tick that brings no pass asks one. So `--rounds` counts passes as before.

### D9. The protocol keeps only the requests that a command sends

`sluice host` answers these requests of today's protocol, each for the command that still sends it:

- `serve`: a submit and each other change tell `sluice host` of work. `sluice-host-does-every-change` moves the changes into `sluice host`.
- `step`: `sluice step` runs one round in `sluice host`.
- `pass`: a follower asks a pass at each tick where `sluice host` passes only when asked.
- `wait`: a wait asks each step (D8). This request is new.
- `hold`: `sluice exec`, `sluice preview` and a pool's job hold room for a run.
- `results`: the session run of `sluice exec` reads its results. `sluice-host-does-every-change` moves `sluice exec` into `sluice host`.
- `hand-over`: a newer command has an older `sluice host` hand over.

`read`, `follow` and `room` go, with the answers `read` and `room`. No command of this release sends them. The reads use the requests of D1. The watch, the wait and `sluice run` use the subscription of D6. `sluice host` renders the room with the overview.

`sluice host` writes no file under `readers/`. It writes no reader's document, no reader's version, lease or lock, and no driver's lock. The document of each repository stays in `sluice host`, and a client gets it only on a subscription. No command reads a file under `readers/`. `sluice host` does not look for an older `sluice run` that holds a driver's lock. It moves each repository that it holds.

`sluice host` answers a request that it does not know with an error, as `host_process/mod.rs:answer` does today, and adds the sluice to run:

```json
{"error":"sluice 0.97.0 asked what this host process, sluice 0.98.0, does not know: {\"ask\":\"read\",...}. Run sluice 0.98.0."}
```

An older sluice program that sends `read`, `follow` or `room` gets that error, and acts on it as its own code acts on an error of `sluice host`. A newer command that reaches an older `sluice host` has it hand over first (`host_process/client.rs:Client::reach`), so a newer command asks a `sluice host` of its own release. An older `sluice host` that does not hand over answers a new request with an error. The newer command then exits 3 and names the version of that `sluice host`.

Before: `sluice host` answered each request of each earlier release, and kept the files under `readers/` for the watches, waits and `sluice run`s of earlier releases. Now: it answers the requests that a command of its own release sends, and keeps no file under `readers/`. This rests on the amendment `never-break-an-existing-client@b539ef8e2413`: before the first public release of sluice, a change may break a client.

### D10. The joined reads and the subscription are ready for the weir crate

`tokio-streams-before-rxrust@0b9fc11cda8b` says that one weir crate gives the skeleton of a resident, from the host process of sluice. `flume host` needs the same joined reads and the same subscription. So the code of D5 and D6 lives in two modules that name no type of sluice:

- `host_process/joined.rs`: `Joined<K, V>`, generic over the key and the answer.
- `host_process/subscription.rs`: the connection's loop of D6. It is generic over a trait, `Serves`, which resolves a folder to a document channel, an optional stream of reports and a guard that the loop drops at a `remove`. sluice's implementation of `Serves` stays in `host_process/mod.rs`.

The two modules use only tokio, tokio-util, tokio-stream, futures-util and serde_json. A later change moves them into the weir crate, with no change of their code, and sluice then uses them from there. This change builds no weir crate.

## Libraries

No new crate comes into the build. One crate that the build already holds becomes a direct dependency of the `sluice` crate. Each version is the one that `Cargo.toml` pins, with the version that `Cargo.lock` holds.

| Crate | What this change uses it for | New | Pin in `Cargo.toml` (`Cargo.lock`) |
|---|---|---|---|
| tokio | the runtime, `spawn_blocking`, the `watch`, `mpsc` and `oneshot` channels, `select!`, `timeout_at`, `Instant`, and `start_paused` and `advance` in tests | no | `1` (1.53.2) |
| tokio-util | `TaskTracker`, `CancellationToken`, and `LinesCodec` in `FramedRead` and `FramedWrite` | no | `0.7`, features `rt` and `codec` (0.7.20) |
| tokio-stream | `StreamMap`, `WatchStream`, `UnboundedReceiverStream` and `StreamExt` | no. The change turns on its feature `sync`, which `WatchStream` needs | `0.1` (0.1.19) |
| futures-util | `SinkExt::send` on a `FramedWrite` | a new direct dependency. The build already holds it through tokio-util, so it is no new crate | `0.3`, no default features, feature `sink` (0.3.34) |
| notify-debouncer-full | the file events that start a reading, as the edge's D4 has them, with no change | no | `=0.6.0` (0.6.0) |
| serde_json | the lines of the protocol | no | `1` |

The clients of D7 and D8 use std alone.

## Claims

Each claim that binds sluice, and what this design does with it.

| Claim | This design | Where |
|---|---|---|
| `a-build-takes-its-version-from-its-tree@b97cf1919925` | Does not touch it. | — |
| `a-decision-that-changes-a-rule-says-before-and-now@87e920a42421` | Keeps it. Each decision that changes a rule says Before and Now in plain words, and the record's amendment does too. | D4, D9; task 6.1 |
| `a-leaked-child-fails-its-test@9a82a5147ba6` | Keeps it. Each test that starts `sluice host` gives it a state folder under the test's folders, so the guard of `sluice-host-owns-its-workers` finds and stops it, and fails a test that leaves a read's git command. | Tasks |
| `a-release-tags-a-release-commit-off-main@c2c371868638` | Does not touch it. | — |
| `clients-never-fetch-or-start-a-tool@8aa9c57f19b2` | Keeps it. A read command runs no git and fetches nothing. A client starts only `sluice host`, on a bare install, as today. | D2, D7, D8 |
| `each-part-is-a-moon-project@561ca40fb85c` | Does not touch it. The change adds no part. | — |
| `each-tracked-file-has-a-formatter@7bd720a7a6c1` | Does not touch it. The change adds no file type. | — |
| `main-names-version-0-0-0@beed3874e15c` | Does not touch it. The versions in the examples are sample versions of an error. | — |
| `names-use-the-domains-terms@343c9fac7fc5` | Keeps it. The new names are `subscribe`, `add`, `remove`, `document`, `pass`, `output` and `--stdin`. `boom terms check` runs on each commit. | D1, D6, D7 |
| `never-break-an-existing-client@ed293965fc5c` | Changes it, by the amendment `never-break-an-existing-client@b539ef8e2413`, which is accepted. Before: a newer `sluice host` answered each request that an older sluice command sends. Now: before the first public release of sluice, `sluice host` answers only the requests of its own release, and names the release to run for any other. | D9 |
| `no-credential-in-the-repository@a349a974ad13` | Does not touch it. | — |
| `no-task-reads-markdown@a25dda6425d7` | Keeps it. No task checks the site's page that the change edits. A person reads the diff. | Task 6.2 |
| `nothing-commercial@80a056165cac` | Does not touch it. | — |
| `nothing-detached@f807c7f5fcea` | Keeps it. A read's git commands are workers of `sluice host`. A client's threads end with the client's process. | D3, D7 |
| `one-forge-read-per-repository-per-interval@8ad9e918d603` | Does not touch it. A status asks the forge from `sluice host` as the command asked it. The bolt's unit `sluice-host-is-the-only-github-client` bounds the forge reads. | Non-Goals |
| `one-github-client-per-host@786dccccffe1` | Keeps it for the reads: a status's forge read moves into `sluice host`. The changes, which still ask the forge from a command, move in the bolt's unit `sluice-host-does-every-change`. | D1, D2 |
| `one-read-in-flight@e99acda774d0` | Keeps it. Asks of one key join one call, and the newest answer replaces the older ones on a `watch` channel. | D5, D6 |
| `one-resident-per-tool@dc963165544c` | Keeps it. The clients do no work. A watch, a wait and `sluice run` print what `sluice host` sends. | D2, D7, D8 |
| `one-subscription-per-plugin@dcc7b7fec9ee` | Keeps it. One subscription serves many folders, so a plugin can hold one. The plugins change in the bolt's unit `the-plugins-hold-one-subscription-each`. | D6, D7 |
| `one-task-per-check@1c550fb50c1d` | Does not touch it. Each check of the tasks is a test that a task already runs. | Tasks |
| `open-changes-archive-cleanly@de24a2987568` | Keeps it. The archive check runs before each commit. | Tasks |
| `plugins-come-from-the-tools-flake@78305fd0bae0` | Does not touch it. | — |
| `reads-go-through-the-resident@69134e5825d7` | Keeps it. `sluice host` answers each read from the state that it keeps, and a command reads nothing itself, past the bound too. | D1, D2, D3, D4 |
| `residents-ask-each-other-by-socket@1c25d836b9e1` | Keeps it. `flume host` gets each status and overview from the socket, and many folders on one subscription. | D1, D6 |
| `residents-do-the-network-and-git-work@a2d68cf95ffe` | Keeps it. The read commands' git work moves into `sluice host`, and the clients keep no state of their own. | D2, D3 |
| `rule-details-break-then-not-then-source@c31fda3f2059` | Keeps it. The rules that task 6.1 rewrites list what breaks them, then what does not, then their source. | Task 6.1 |
| `rules-name-rules-by-their-words@1b08e8dc3be7` | Keeps it. The rewritten rules name other rules by their words. | Task 6.1 |
| `runs-max-counts-checks-only@4276e56de833` | Keeps it. A read's git commands take no slot of `[runs] max`. | D3 |
| `the-gate-validates-specs-strictly@d8e057526e31` | Keeps it. `openspec validate --strict` runs before each commit. | Tasks |
| `tools-at-their-pinned-versions@274efe1244ff` | Keeps it. Each crate is the version that `Cargo.toml` and `Cargo.lock` pin. | Libraries |
| `workers-end-with-their-owner@292e869158d6` | Keeps it. A read's git commands start through `crate::workers`, each in a process group of its own. | D3 |
| `workers-within-their-tools-limit@b48467b4ed47` | Keeps it, as `sluice-host-owns-its-workers` reads it: `[runs] max` limits the checks, and the blocking pool's 256 threads bound the git commands of the reads. | D3 |
| `flume-has-a-resident@49876bc5c73c` | Keeps sluice's part: `flume host` asks `sluice host` through its socket. | D1, D6 |
| `one-worker-limit-per-tool@e651e72ce246` | Keeps it, as `workers-within-their-tools-limit@b48467b4ed47` does. | D3 |
| `sluices-runs-end-with-its-host-process@e814b716f3b1` | Does not touch it. A read starts no run. | — |
| `the-runner-serves-a-socket@66058c4ecc13` | Does not touch it. A status's forge read in `sluice host` gets its token as `sluice host` gets it today. | — |
| `the-runner-syncs-the-drives-log@b6de499b3259` | Does not touch it. | — |
| `the-service-manager-starts-each-resident@d0e66e466414` | Does not touch it. A client reaches `sluice host`, and starts it on a bare install, as today. | D2 |
| `tokio-streams-before-rxrust@0b9fc11cda8b` | Keeps it. The reads and the subscription use tokio's `watch` channel and tokio-stream, at the edge alone. The core stays synchronous, and the code of D5 and D6 is ready for the weir crate. | D3, D5, D6, D10, Libraries |

## Risks / Trade-offs

- [A read now costs a round trip to `sluice host`, and its rendering runs there.] → A one-shot read already waited for a reading from `sluice host`. The render is the same code, and it runs once for asks that share a read.
- [A `sluice host` that is wedged makes every read exit 3.] → Each read has its bound, and past it `sluice host` answers from its last state. Only a `sluice host` that does not answer the socket makes a read fail, and the next command reaches it again or starts one.
- [A read's call and the worker read one repository at one time.] → The read's call only reads the records and the branches as the host holds them, as a one-shot command did beside the worker before.
- [A read's call makes a binding each time.] → A one-shot command made one each time too. The asks that share a read share one call and one binding (D5).
- [A subscription to many folders holds many repositories read.] → It holds each one as one follower did. A repository with nothing in play is still asked at most once a minute.
- [An older sluice program on the host fails against the new `sluice host`.] → Its error names the sluice to run. No public release of sluice has clients, so only the host's own older programs meet it.

## Migration Plan

- The first command of this release reaches an older `sluice host` and has it hand over, as each newer command does. After that, each read goes by the new requests.
- A watch, a wait or a `sluice run` of an earlier release that runs across the upgrade gets the error of D9 at its next tick. The error names the sluice to run.
- The files under `readers/` that an earlier `sluice host` wrote stay until the host's state is removed. Nothing reads them.
- Nothing in the records changes form.
