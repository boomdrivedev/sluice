# Design

## Context

Record 0122 has two halves. This change builds the first: a queue's
home written in its records, a move between homes, and one lead while a
run is live, with record 0093's amendment of 2026-11-08 on who may lead
(58, 72, 135, 137, C.6 as record 0122 amends it, C.8). The second half,
a run's life, is `a-runs-life-is-written-first-and-actions-runs-its-checks`.

What the code does today, at d6d11623:

- **Routing.** `Records::route` (`crates/sluice/src/profiles/records.rs`)
  reads the branch's policy through `Branches::policy_of` and routes a
  branch to `remote refs` where it names signers and a remote, to
  `local files` otherwise, keeping the answer in `routes` for the
  binding's life. `lock`, `read`, `write`, `allocate_id` and every other
  call of the `State` seam go through it.
- **The move.** `Records::migrate`, called under the branch's lock the
  first time a binding routes it to its remote, pushes this host's own
  records there when the remote holds none and otherwise notes this
  host's queued submissions in `sluice-remote/moved/<branch>.json`; the
  round's `adopt_left_behind` (`crates/sluice-core/src/step.rs`)
  submits them again under new numbers, each naming its old one in
  `was`, and `Sluice::moved_to` (`ops.rs`) finds the new number. Nothing
  moves a branch back.
- **The lead.** `Sluice::keeps` returns true for a branch whose policy at
  its revision names no signer. Otherwise it takes the lead where none
  is named, or where the records' version has stood unchanged for the
  lease by this host's own observation and the branch has work
  (`has_work`). It never asks whether the branch accepts this host's
  signature. The lead's round writes only what changed, so a run longer
  than the lease leaves the version standing, and `has_work` is true
  while a run is under test: another instance takes the lead under it.
  The new lead's `poll` reads the old lead's own claims, whose host never
  beat while it led, as lapsed after `[peers] alive`, and the time limit
  read from the combination's `opened` then writes the open run timed
  out. That last part is the run's life change's (Out of scope); this
  change removes the lead moving.
- **Who claims.** `may_claim` and `counts_here` already let only a host
  whose signer the gate trusts, with its key listed (`key_listed`),
  claim a run of a branch whose records are on its remote. Intake asks
  `keeps` (`pulls.rs` `takes`), so a host that leads takes requests in.

## Goals / Non-Goals

**Goals:**

- One home per branch, written in its records, which every process reads
  and none works out from a policy it read, a branch with no home
  anywhere aside.
- A move that is written step by step, waits for the combinations under
  test, keeps every live submission's number and state, and that the
  next round finishes wherever the last one stopped.
- A wait and a status that follow a submission to where it lives.
- A lead only among the hosts the branch accepts, which never lets its
  lease pass while a run of the branch is claimed or under test.

**Non-Goals:**

- A run's states, its own time limit, a claim that lapses only by its
  own lease, and the lead's sign of life (the run's life change).
- Which remote a branch's revision is read from (rule core.27): the
  branch seam reads it as today.
- Mapping records of the older shape: a host's queues are drained before
  the release that brings this (principle 12).

## Decisions

### D1. The home is `home.json`, in each home of the branch

Each home of a branch holds `home.json` beside `branch.json`: in local
files `<common>/sluice/<branch>/home.json`, in remote refs a file of the
state commit's tree. It is written with the records it belongs to, in
one write (63).

```json
{
  "schema": "sluice/home@1",
  "at": "origin",
  "chosen_at": "4f6fe55f…",
  "state": "open",
  "moving": {"to": "here", "chosen_at": "9a1c…", "since": "2026-11-14T10:00:00Z"},
  "came_from": {"home": "here", "host": "chuck-sluice-alpha", "carried": {"52": 52, "53": 61}},
  "closed": {"to": "origin", "carried": {"52": {"number": 52}, "53": {"number": 61}}}
}
```

`at` names the home, `here` for the host's own files or the name of a
remote. `state` is `open`, `moving` (with `moving`) or `closed` (with
`closed`). `came_from` is written by the move that opened it. In a closed
remote home whose branch came home, `closed.to` is `here` and each
carried entry names the host that took it as well as its number there.
`sluice-core::records::HomeDoc` reads and writes it; the records binding
reads it to route and the core reads it, as `Line::home`, to decide.

*Not taken:* a field of `branch.json`, which the binding would have to
parse beside the core's document; a ref of its own per home, which a
write of the records would not move with them (63).

### D2. Every process reads the home from the records

`Records::route(branch)` reads, in order:

1. this host's own `home.json` of the branch: `open` or `moving` routes
   to local files, and `closed` to the home its pointer names;
2. where this host has none, the `home.json` of the branch's state on
   each remote that may hold it, the branch's remote as rule core.27
   reads it and each remote this host has fetched a state of the branch
   from, as the host process's tick last fetched it (129): one that is
   `open` or `moving` is the home;
3. where neither holds one, the branch has no home, and is routed by its
   policy at its head as today (signers and a remote: that remote; local
   files otherwise). The first write into a home that holds no
   `home.json` writes it, `open`, naming the home and the revision whose
   policy chose it.

A home reached through a closed home's pointer that is itself closed
toward `here` routes to local files (D4). Two homes both `open` or
`moving` refuse the branch alone, naming both (26). The route is read at
every lock, read and write within a pass and again at the next pass
(`next_pass`), never kept for a binding's life, so the host process and
a one-shot command read one answer.

*Not taken:* routing by the head's policy for every process (record
0116's half), which is one answer per read but still each process's
own, and makes a move a side effect of whichever read first sees a new
policy.

### D3. A move is three writes, decided by the round that keeps the home

The home a branch's policy at its head names is computed by the core
from the rules it already reads there: `signed` and a `remote` name that
remote, anything else `here`. A round that keeps the open home, the lead
of a remote home or this host for its own files, and reads a policy at
the head naming another home:

1. **writes `moving`** in the current home, with the target, the
   policy's revision and the time, and logs `moving`. From then its
   rounds form and open no combination: combinations under test go on,
   their verdicts are read, what passed lands, outcomes are told and
   checkouts released. Formed combinations not yet under test are
   dropped. Submissions are still taken into the moving home, and carried.
   A head whose policy names the open home again clears `moving` and
   logs `stayed`.
2. **carries**, once no combination is under test and no landing is
   unsettled: the round calls `State::carry(branch)` under the branch's
   lock, and the binding writes the new home in one write: every live
   submission, its number and its state, the deliveries store whole
   (core.52), and `home.json` `open` with `came_from`. The core logs
   `carried` with each number it had and has.
3. **closes** the old home: `home.json` `closed` with its pointer, in a
   second write.

A round that stops after 1 finds `moving` and carries when the runs have
ended. One that stops after 2 finds the new home's `came_from` naming the
old home, which is still `moving`, and closes it. `carry` therefore reads
the target before writing it and writes only what it has not written.
Finished submissions and the log stay in the old home (D7).

A carry by a host whose signature the branch does not accept is still
made: carrying its own submissions into the remote is submitting them
(D8).

*Not taken:* one write across both homes, which no profile can make;
63 holds within a home.

### D4. A branch comes home to each host's own files

A branch in the hosts' own files has one home on each host (C.6 local
files: one host), so a remote home's submissions go home to the hosts
that made them. The lead's carry, once no combination is under test,
closes the remote home with `closed.to = here` and carries the live
submissions whose `from.host` is its own. Every other host whose binding,
under the branch's lock, reaches that closed home holding live
submissions of its own not yet carried carries them into its own files,
`open` with `came_from`, then writes each one's pointer in the remote
home. A submission naming no host, or a host that never returns, stays
in the closed remote home, readable, and its status says it waits for
its host to take it.

*Not taken:* the lead carrying every submission into its own files
(record 0116's way), where no other host could read its own submission
(26).

### D5. A carried submission keeps its number, or takes the next

A carried submission keeps its number where the new home's branch holds
no submission of that number, live or finished, and the new home's
count, `refs/sluice/ids` on a remote and `.next-id` on the host, is
raised past the highest number carried. A number the branch already
holds there, as when two hosts' own queues each held #52, is carried
under the new home's next number: the submission's record names its old
number in `was` and the home it came from in `carried_from`, and the old
home's pointer and the `carried` line name both. An owed delivery of a
submission carried under another number is carried under a key naming
the number it has now (core.52).

*Not taken:* every submission under a new number (design D10 of ADR
0037's build, and record 0116), which record 0122 replaces; numbers per
branch, which record 0114 did not choose and which two hosts' own queues
would repeat all the same.

### D6. A wait and a status follow a submission to where it lives

`Reading::read` (`crates/sluice/src/lib.rs`) keeps the home each
submission was last read in. Where that home has closed, it reads the
pointer and goes on with the submission under the number it has in the
new home, which the wait's document then gives with `was` and
`carried_from`. `sluice status <n>`, and the lookup every act by number
makes (record 0114), reads the open home first; a number it does not
hold there, live or finished, is read in each closed home this host
reached it through, and one the pointer carried under another number is
named as it is now. A wait never works out what carried its submission:
it reads the pointer.

### D7. A closed home keeps its history

A closed home keeps its finished submissions, its index and its log, and
nothing writes them again but its pointer. A status of a submission that
finished before the move reads it there, so the move carries no history
and no line of a log is written twice (core.41).

### D8. Only a host the branch accepts leads a remote home

`Sluice::keeps` reads the home first. A branch whose open home is this
host's own files is kept by this host, as before. A remote home is kept
only by its lead, and an instance takes or keeps the lead only where the
policy at the branch's revision accepts its signature: its signer is one
the gate trusts (`rules.claims`) and the records say the key it signs
with is listed (`verifies_own`, as `key_listed` asks). A policy at the
revision that names no signer, as when the branch is coming home, lets
any instance lead, through the lease as before, so one carries the queue
home. An instance the branch does not accept never writes `lead`: its
round is led, beats, levels its copy and claims nothing (`counts_here`),
intake reads it as not keeping the branch and takes nothing in (137),
and its submit writes its submission and returns. A lead the branch no
longer accepts writes nothing as lead from its next round, so an
accepted instance takes the branch once the lease passes (73).

The conformance tests that opened a run with a lead signing as no one
the gate trusts, so another host would claim it, open it with a lead
signing as `box` that has no room.

*Not taken:* an unaccepted lead keeping the rounds while accepted hosts
run the checks, the split record 0093's amendment rejects.

### D9. The lead writes at least once a lease while a run is live

The branch document gains `kept`, the lead's own time of its last write.
Every write the lead makes sets it. A round the lead keeps in which a
run of the branch is claimed or under test, held by any host, or open on
a combination under test, and which writes nothing else, writes `kept`
alone once half a lease has passed by the lead's own clock since the
`kept` it wrote. Other instances measure the lease as before, from when
they first saw the version stand (73), so they never compare clocks. The
host process steps a branch with a run under test every pass, so half a
lease leaves the lead a margin of many passes.

*Not taken:* the lead's sign of life on `refs/sluice/hosts/<host>`
standing for its lease, which is the host's and not the branch's (72
keeps a branch by writing its records); a `kept` line in the log, which
would add a line every half lease of every run.

### D10. No setting names how a queue runs

Nothing is added to a host's settings or a branch's policy, and both
already refuse a key they do not know. Two tests pin it: a host's
settings with `[peers] lead`, or a `[queue]` table, and a policy with a
branch's `lead` or `runs` key, are each refused naming the key.

### D11. The status and the overview name the home

The overview document's branch gains `home`: `at`, and while moving
`moving` with its target, since when, and the combinations under test it
waits for. `sluice status <branch>` says `on origin`, `in this host's own
files`, or `moving to origin since <time>, waiting for 2 combinations
under test`.

### D12. The left-behind design goes

`Records::migrate`, `Moved`, the `sluice-remote/moved/` notes,
`State::left_behind`, `State::forget_left_behind`, the round's
`adopt_left_behind`, the fakes' `leave_behind`, and the tests of them go.
`was` stays, written only by D5's carry.

## Looking across the family

- **weir.** The deliveries store is the branch's live records (core.52),
  so the move carries it whole as files and reads no delivery by a rule
  of its own; weir's pack holds over the carried store as over any.
- **Flume and crew** read a wait's document. A carried submission's
  document is its record in the new home, naming `was` and
  `carried_from` where its number changed, so a place following a number
  is told the number it has now.
- **Boomdrive's runner** keeps no queue home and is not changed. The one
  owner of where a queue lives is sluice's records.

## Risks / Trade-offs

- **A move waits for every combination under test.** A long gate delays
  the move by its length, with nothing new opened meanwhile. Record 0122
  accepts it.
- **A host that never returns keeps its submissions in a closed remote
  home.** They are readable there and named as waiting for their host,
  which takes them home the next time it runs sluice.
- **A branch coming home whose policy is refused** (a `pull_requests`
  table with no signers, 137) runs no round, so its move waits until the
  policy is fixed, as every round of a refused branch does.
- **Writes every half lease** on a remote while a run is live: one push
  each two and a half minutes at the default lease.

## Migration Plan

A host's queues are drained before the release that brings this. A
branch's records with no `home.json` have no home, and the first write
writes the one its policy chooses (D2); the `sluice-remote/moved/` notes
are read by nothing. The build amends record 0122 with these choices.

## Open Questions

None.
