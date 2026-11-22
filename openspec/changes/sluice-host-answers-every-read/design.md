# Design

## Context

After `sluice-host-runs-on-a-tokio-edge` and `sluice-host-owns-its-workers`, `sluice host` keeps one scheduler for each repository, with one reading in flight at a time and a `watch` channel of its readings. Its workers are its children.

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

### D2. A client sends its folder and binds nothing

`main.rs` sends a read command to `sluice host` before it binds a repository. The client reaches `sluice host`, starting it on a bare install as today, sends its folder, and prints the answer. It runs no git, and opens no file of a repository or of the records. Its folder is its working folder, or `--worktree` where the command takes one.

`sluice host` resolves a folder with a git worker: the repository's common folder, the worktree, its branch and its head. A folder in no repository gets the error that the command gave before, with its exit code. A folder that is no worktree, such as a bare repository, resolves as the command resolved it before.

The tests' host process, which answers in the test's own process, answers the new requests through the same `HostProcess` trait (`host_process/ask.rs`).

### D3. A read is answered on a reading thread beside the worker

Each repository's scheduler gets a reading thread beside its worker thread. The reading thread holds a binding of the repository that holds what the host holds and fetches nothing, as a one-shot command's binding did. It answers each read after the scheduler's current reading, from the records and the branches as the host holds them. A step or a landing on the worker thread does not hold a read back. A git command that the reading thread starts is a worker of `sluice host` (`workers::command`).

A read of results asks the host process's index of results, as `results` does today. `sluice inbox` reads what is owed and records each hand-over, as the command did, and `--confirm` confirms in the same call.

### D4. A read past its bound is answered from the last state

A read waits for a current reading within the round's retry bound and a tick, as today. Past that, `sluice host` answers from the records and the branches as it last held them, and puts in `stderr` when it last read the repository. Where no `sluice host` answers, or it ends before it answers, the client exits 3 and says that sluice's host process did not answer. The client reads nothing itself.

Before: past the bound, or with no host process, the command read the records and the results itself. Now: `sluice host` answers from the state that it last kept, or the command exits 3. (`reads-go-through-the-resident@69134e5825d7`)

### D5. Asks of one read share the read in flight

The scheduler keeps one read in flight for each key: the folder's resolution, keyed by the folder, and each read, keyed by its request. An ask whose key is in flight joins that read and gets its answer. An ask that comes after a command told of a change starts a new read once the one in flight ends, and the asks that come meanwhile join that new read. The readings of the repository stay one in flight, as the tokio edge made them. The newest answer of a key replaces the older one. (`one-read-in-flight@e99acda774d0`)

### D6. A subscription serves many folders on one connection

`subscribe` names the folders to serve. The connection stays open. The client sends `{"add":"<folder>"}` and `{"remove":"<folder>"}` lines on it. An `add` line can name how the client follows the folder, as `as`: `watch`, which is the default, `wait` or `run`. `sluice host` sends a `document` line for a folder when the folder is added, and after that each time the folder's overview changes in any part but its time:

```json
{"add":"/w/sluice/feat","as":"wait"}
{"document":{"folder":"/w/sluice/feat","overview":{"schema":"sluice/overview@1","branches":[...],...}}}
```

The three ways keep what the kinds of today's `follow` did. While a watch follows a repository with work in play, `sluice host` reads it each second, and three seconds apart otherwise. A watch's or a wait's addition asks the remote at once. A folder added as `run` gets a `pass` line with each pass's report, in order, and no `document`. `sluice run` adds its folder as `run`, and prints each report as it prints a pass today:

```json
{"pass":{"folder":"/w/sluice/main","report":{"lines":["main: #4 landed as 9c1e2f0"],"failed":[]}}}
```

The overview of a folder is the reader's document of its repository, as `sluice overview --json` prints it. Each folder resolves to its repository by D2. The repositories' scheduler sends each new overview on its `watch` channel, and the connection's task reads the newest value of each channel. So a slow client gets only the newest document of each folder. Two folders of one repository share its reading.

While a subscription is open, `sluice host` keeps each of its repositories read, and the subscription counts as a follower, so `sluice host` does not end. A folder whose repository is gone, or has no sluice, gets the document that says so, and is removed from the subscription. The connection ends when the client closes it or `sluice host` stops. (`flume-has-a-resident@49876bc5c73c`, `one-read-in-flight@e99acda774d0`)

Not taken: one connection for each folder. A plugin that serves many repositories then holds many connections, which `one-subscription-per-plugin@dcc7b7fec9ee` rules out.

### D7. `sluice overview --json --watch --stdin`

`--stdin` needs `--watch`. The command opens one subscription with no folder. It reads its standard input: a line `add <folder>` adds the folder, and a line `remove <folder>` removes it. A relative folder is read from the command's working folder. It prints each document as one line, with `folder` in it. It ends when its input closes and no folder stays, or when it is stopped.

`sluice overview --json --watch` with no `--stdin` is a subscription of its working folder. Its lines are as today, with no `folder` field.

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

### D8. A wait follows a subscription

`sluice wait`, a waiting `sluice submit` and `wt submit` open a subscription of their folder as `wait`, and read their submission, its combination's progress and its branch's hold from each document. Where a document no longer holds the submission, the wait asks `status` of its number once for that document, and reports its outcome from the answer. A wait that follows a later submission or a request asks `sluice host` in the same way. A wait still asks a pass at each tick where `sluice host` passes only when asked.

### D9. The protocol keeps only the requests that a command sends

`sluice host` answers these requests of today's protocol, each for the command that still sends it:

- `serve`: a submit and each other change tell `sluice host` of work. `sluice-host-does-every-change` moves the changes into `sluice host`.
- `step`: `sluice step` runs one round in `sluice host`.
- `pass`: a follower asks a pass at each tick where `sluice host` passes only when asked.
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

Before: `sluice host` answered each request of each earlier release, and kept the files under `readers/` for the watches, waits and `sluice run`s of earlier releases. Now: it answers the requests that a command of its own release sends, and keeps no file under `readers/`. This rests on the amendment `never-break-an-existing-client@3e4073c64ec2`: before the first public release of sluice, a change may break a client.

## Risks / Trade-offs

- [A read now costs a round trip to `sluice host`, and its rendering runs there.] → A one-shot read already waited for a reading from `sluice host`. The render is the same code, and it runs once for asks that share a read.
- [A `sluice host` that is wedged makes every read exit 3.] → Each read has its bound, and past it `sluice host` answers from its last state. Only a `sluice host` that does not answer the socket makes a read fail, and the next command reaches it again or starts one.
- [The reading thread and the worker thread read one repository at one time.] → The reading thread only reads the records and the branches as the host holds them, as a one-shot command did beside the worker before.
- [A subscription to many folders holds many repositories read.] → It holds each one as one follower did. A repository with nothing in play is still asked at most once a minute.
- [An older sluice program on the host fails against the new `sluice host`.] → Its error names the sluice to run. No public release of sluice has clients, so only the host's own older programs meet it.

## Migration Plan

- The first command of this release reaches an older `sluice host` and has it hand over, as each newer command does. After that, each read goes by the new requests.
- A watch, a wait or a `sluice run` of an earlier release that runs across the upgrade gets the error of D9 at its next tick. The error names the sluice to run.
- The files under `readers/` that an earlier `sluice host` wrote stay until the host's state is removed. Nothing reads them.
- Nothing in the records changes form.
