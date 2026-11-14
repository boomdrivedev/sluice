---
number: 122
title: A queue's home and a run's life are written in its records before anything acts on them
status: accepted
date: 2026-11-13
decision-makers:
- agentplot-design
---

# A queue's home and a run's life are written in its records before anything acts on them

Status: accepted, agentplot-design's.

Sources: the design review of sluice's main at `045cb7d0`, 2026-11-12 (https://claude.ai/artifact/Vz3Q9zgdTaxmxnZJ7pjF5D), on a queue's home decided by each process, a run's state with no one owner, and effects performed before the write that names them; records 0037, 0093, 0114 and 0116; swb-3-ops' reading of switchboard-kit's `refs/sluice/state/bolt/a-function-that-calls-aws-is-proved-on-floci` on 2026-11-13 (the lead moving between chuck-herdr-alpha and chuck-sluice-alpha every 6 to 14 minutes, each move lapsing the other host's live claim and writing a passed run as timed out); agentplot-ops' readings of switchboard-kit's records on chuck-herdr-alpha, 2026-11-12 (a second #52 and #53, a round stalled on its lock, bolts moved between the host and GitHub, twelve unproven runs opened again whole and read as "testing 37m of 25m"); sluice's main at `49527985`: `crates/sluice-core/src/records.rs` (`RunState` is open, starting, testing, passed, failed or timed out; `no_replay` reopens a whole run) and `crates/sluice-core/src/step.rs`; the statement in agentplot/blueprints, `design/sluice/requirements.md`, clauses 53, 60, 62, 66, 71, 72 to 77, 87, 88, 108, 109, 114, 129, 135 and C.6.

## Context and problem statement

Most of sluice's fixes in November went into two places with no design of their own.

- **Where a queue lives is worked out, not written.** Each process works out from the policy it reads whether a branch's records are the host's files or the remote's refs, and keeps that answer. Two processes that answer differently each hold a lock and number entries of their own. Record 0116 routes by the head's policy, one answer per read, but a move between homes is still a side effect of whichever round first reads the change.
- **A run's state has no one owner, and acts come before the write.** A run is made, started, released or reopened by whichever step reaches it, and some of those acts happen before the records say so. An unproven replay reopens the whole run, and status measures a candidate from its first testing line, so a run waiting for room reads as testing past its limit.

## Considered options

1. **Write both in the branch's records: the queue's home as a record every reader follows, and a run's life as states that are written before any act (chosen).**
2. **Keep working them out, and fix each defect as it comes.** This is what November did, and each fix went into code no design described.
3. **One coordinator process per branch that holds both in memory.** It gives one owner, but a host that stops loses the queue's state, and 111 has a submission reach its outcome with no host of its submitter running.

## Decision outcome

### A queue's home is a record

- **One home, written down.** A branch's queue has one home, the host's files or its remote's refs, written in the branch's records with the revision of the policy that chose it. Every process reads the home from the records and never works it out from a policy it read. The lock, the numbers and the entries live in the home and nowhere else.
- **The policy says where it should live; a move takes it there.** When a round reads a policy at the branch's head whose home differs from the recorded one (record 0116's route), it writes a move in the current home and opens no new candidate. Once no run of the branch is under test, it carries every open entry to the new home with its number and its state, writes the new home's record, and closes the old home with a pointer to the new one. Each step is written before the next begins, so a round that stops halfway leaves the move written and the next round finishes it.
- **One lead while a run is live.** A branch held on its remote has one lead at a time (58), which keeps it by writing its records (72). While any run of the branch is claimed or under test, the lead writes at least once a lease, so the lead never lapses under a live run, and an instance whose signature the branch does not accept never takes it (record 0093's amendment of 2026-11-08).
- **Numbers survive a move.** An entry keeps the number it was given, which comes from its branch's own records (record 0114), and a waiter on that number finds it in the new home through the old home's pointer.
- **A wait reads the entry where it lives.** A wait follows the entry's home, and a closed home's pointer, to the entry's recorded state. It never infers what carried the entry.
- **The other direction is the same act.** A branch whose policy stops naming signers comes home by the same move, which is record 0116's half on a branch coming home.

### A run's life is states written before any act

- **One owner.** A run's state is written only in its branch's home, and only by the instance whose claim holds it or the lead that opened it. Nothing acts on a run except from a state written there. A change of lead changes no run: a claim lapses only when its own lease does, and the claim holder's report is taken whichever instance leads, so a green run is never dropped or read as timed out because the lead moved.
- **The states.**

  ```
  open ──claim──▶ claimed ──start──▶ running ──report──▶ ended ──release──▶ closed
    ▲  waiting for an executor            the limit       each check's outcome:
    │  or for room (87, 71)               counts here     passed, failed, none,
    │                                     (66)            or unproven (108)
    └──────────── a follow-up run holding only the unproven checks
  ```

- **Write, then act, and every act can be done twice.** Making a place, starting the checks, releasing the place, telling a session and landing each happen only after the state that names them is written, and each leaves a proof the next reader checks, so an act found done is not done again. A host that stops between the write and the act leaves a state the next reader finishes.
- **The limit is the run's.** A run's time limit counts from its own `running` state. An `open` run is under no limit while a live executor of its system may take it (87). One that no live executor may take is ended at the candidate's limit with no outcome: the combination is not passed, no check is written failed, nobody is blamed, and each check says who turned it down and why, or that no host that may take it was live (66; core.22). Status reads the state: an open run waits for room since when; a running run is testing so long of its limit.
- **An unproven replay reruns alone.** A run that ends with unproven checks keeps every other check's outcome. The lead opens a follow-up run holding only those checks, each run with no replay of its own key, and the candidate's verdict is read over both runs (108, S42). A run's report, a follow-up's as any other's, comes only once the run has ended, and holds an outcome for each check the run was given. A check it holds none for has no outcome, so the combination is not passed and nobody is blamed (60, 114; core.22).
- **A pool's job is a run like any other.** Its start writes the claim and its end the report, through the same states (88), so the pools of record 0119 are built on this.

### How it is built

It is built on record 0093's one host process. The statement's C.6 and the clauses of the run's life are amended to match on agentplot/blueprints. A host's queues are drained before the release that brings it, so no record of the older shape is ever read and no code maps one shape to the other (principle 12).

### Consequences

- The November defects in these areas have nowhere to happen. Two processes can't hold two locks or two numbers for one branch, and a move can't strand an entry. A run waiting for room can't read as timed out, and an unproven replay can't cost a whole gate.
- A move between homes waits for the branch's runs under test to end, and opens no candidate meanwhile.
- The GitHub Actions and CodeBuild pools' jobs (record 0119) report through the same states.

## Amendment, 2026-11-14: what the build of a queue's home chose

The change `a-queues-home-is-written-in-its-records` builds this record's first half. Its choices are the build's own, cheap to change, and hold no choice of the user's. A branch coming home to each host's own files and a carried number that the new home holds taking the next one are this record's own, as boomdrive-design amended it on 2026-11-14 on sluice's main, and are cited here, not restated (the statement's C.6 at agentplot/blueprints `e966dcb`).

- **The home is `home.json`, in each home.** It sits beside `branch.json` in the host's own files and in the remote's state commit, and is written with the records it belongs to in one write (63): `at`, `chosen_at`, `state` (`open`, `moving`, `closed`), with `moving`, `came_from` and `closed` where they apply. *Not taken:* a field of `branch.json`, which the records binding would parse beside the core's document; a ref of its own, which a write of the records would not move with them.
- **The route is read from the records, and the policy only where no home exists.** This host's own `home.json` first, open or moving to its files and closed to its pointer; then a remote's as last fetched, the branch's remote as its policy names it, signers or none, and every remote this host fetched a state from; and only where neither holds one, the policy at the head, whose revision the first write's home names. The route is read at every lock, read and write and kept for nothing past a pass, and a write is refused where a move took the records from the home they were read in. *Not taken:* record 0116's route by the head's policy for every process.
- **One queue in two homes is told apart from a queue not yet moved.** A branch is refused, naming both homes, where this host's own home is open while a live remote home came from this host's own files: the one queue is in two places. A host's own open home beside a remote home another host's queue or a policy opened is a queue not yet moved, and its round moves it. *Not taken:* refusing any two open homes, which would refuse every host whose own queue had not yet moved, so none ever could.
- **A move is three writes, and the round that carries does nothing more.** `moving` is written in the home, nothing is formed while it stands and nothing lands beside, and a queued submission reads queued; once no combination is under test or judged and no landing is unsettled, the round carries under its lock and ends, the next forming in the new home. The `carried` line is written in the new home's own write, so it is written once with what it names. The core decides once what every carry writes, and each records binding reaches its homes through it. A carry reads its target first, finding what an earlier carry wrote by its work, head and time of submission, live or finished, so one stopped between its writes is closed by the next with nothing carried twice. *Not taken:* one write across both homes, which no profile can make; logging `carried` in a write after the carry, which a carry stopped between would write twice or not at all.
- **Both of a branch's locks are held together.** A round holds the branch's lock in the host's own files and its remote lock at once, so whichever home a move leaves the records in, they are written under the lock the round took (rule core.14). *Not taken:* the lock of the home routed to, which a move would change under the round.
- **A remote state is held as this host last wrote it.** A write of a remote's state sets this host's copy of it as fetched to the commit it pushed, so a read with no fetch reads its own write, the home among it.
- **A submission is read where it lives, the pointer first.** A status, a wait and every act by number read, on each branch, the pointer of each closed home this host reached the branch through before the open home, then the open home, then the closed home's own live or finished records: a number carried under another is held in the open home by the submission it gave way to, so the pointer must decide. A wait reads its submission again by number whenever its queue's document holds another submission under that number. *Not taken:* the open home first, which would answer another host's submission for a number given up; a wait that works out what carried its submission.
- **The closed home keeps its history.** Its finished submissions, its index and its log stay, and nothing writes them again but its pointer; the carried submissions' live documents leave it with its closing write.
- **Only a host the branch accepts leads a remote home.** It signs as a signer the policy at the branch's revision trusts, with a key the records say it lists; where that policy names no signer, any host leads through the lease, so one carries the queue home. A lead with no room of its own opens its combinations all the same, their runs open to every host the branch accepts. *Not taken:* an unaccepted lead keeping the rounds while accepted hosts run the checks, which record 0093's amendment rejects; a lead that opens nothing while it has no room, which would leave a branch led by a host with no room untested.
- **`kept` every half lease.** The branch document's `kept` is the lead's own time of its last write; every write the lead makes sets it, and while a run of the branch is claimed or under test it writes `kept` alone once half a lease has passed by its own clock. Other hosts measure the lease as before. *Not taken:* the lead's sign of life standing for its lease, which is the host's and not the branch's; a `kept` line in the log every half lease of every run.
- **The left-behind design goes.** `migrate`, the host's `sluice-remote/moved/` notes, `State::left_behind`, `forget_left_behind` and the round's adoption of what was left behind are gone; `was` is written only by a carry, beside `carried_from`.
