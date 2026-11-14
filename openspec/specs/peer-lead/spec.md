# peer-lead Specification

## Purpose
Lets any host running sluice advance a branch whose state is on its
remote, one at a time, and lets another take over when it stops or goes
quiet, so a submission reaches its outcome with no host of its submitter
running (72 to 74, 111).

## Requirements

### Requirement: A host keeps a branch only by writing its state
Only the host named as the branch's lead in its state SHALL advance the
branch. A host SHALL become the lead by a write to the branch's state
naming it, and SHALL stop advancing the branch as soon as a write of its
own is refused and the state it reads names another lead (72, 58).

#### Scenario: The former lead stops after a takeover
- **WHEN** host B takes `main` from host A, and host A then tries to write `main`'s state
- **THEN** host A's write is refused, host A reads host B as the lead, and host A advances `main` no more

### Requirement: Another host takes a branch after the lease
A host with work to do on a branch SHALL take it once it has itself seen
the branch's state unchanged for the lease. It SHALL measure the lease
on its own clock from when it first read that version, never from a
time another host wrote, and SHALL treat a stopped lead and a quiet one
alike (73).

#### Scenario: A lead that went quiet
- **WHEN** `main`'s lead is alive but has written nothing for longer than the lease, and host B has a submission waiting on `main`
- **THEN** host B takes `main` and advances it

#### Scenario: Clocks that disagree
- **WHEN** host A's clock is an hour ahead of host B's
- **THEN** host B takes `main` exactly one lease after it first saw `main`'s current version, whatever times host A wrote

### Requirement: The lease and the sign-of-life bound are the host's settings
The lease and the bound on a host's sign of life SHALL be set in the
host's config under `[peers]`, as `lease` and `alive`, one value each for
every branch the host serves. A branch's policy SHALL NOT set either,
and SHALL bound only how long a combination's checks may run (66, 74).

#### Scenario: A policy that sets the lease
- **WHEN** `.config/sluice.toml` has `lease = "1m"` under `[branches.main]`
- **THEN** the policy is refused, and the message names `lease` and the host's `[peers]` table

#### Scenario: Defaults
- **WHEN** the host's config has no `[peers]` table
- **THEN** the lease is five minutes and the sign-of-life bound is two minutes

### Requirement: A landing lands once whoever lands it
A landing started by one lead and found by the next SHALL be settled by
reading the branch on the remote: recorded landed if the remote holds
the landing commit, formed again otherwise. A submission SHALL be landed
at most once whichever host's push the remote accepts (S17).

#### Scenario: A takeover mid-landing
- **WHEN** host A wrote a landing intent for a submission, went quiet, and host B took `main`
- **THEN** the submission lands exactly once, by whichever push the remote accepts, and both hosts read it landed

### Requirement: A submission goes on without its submitter's host
A submission to a branch whose state is on the remote SHALL push its
head and write its entry to the remote's state, then wake a host or pool
able to advance the branch. It SHALL reach its outcome with no host of
its submitter running whenever one is (111).

#### Scenario: Submit and close the laptop
- **WHEN** a session submits to `main` and its host is switched off straight after
- **THEN** another host or a pool's job advances `main`, the submission lands, and its session reads it landed when its host returns

#### Scenario: A waiting submit runs rounds
- **WHEN** a session runs `sluice submit main` and waits, and no other host is leading `main`
- **THEN** the waiting submit takes `main` and runs rounds while its host is up, as any lead does

### Requirement: A lost wake loses nothing
A wake that never arrives SHALL only delay a submission: the state on
the remote SHALL hold everything needed for the next host or job that
reads it to advance the branch (59).

#### Scenario: The wake is lost
- **WHEN** a submission's wake fails to reach any host or pool
- **THEN** the submission stays queued in `main`'s state, and the next `sluice run` or waiting `sluice submit` that reads `main` advances it
