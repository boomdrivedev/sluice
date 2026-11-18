---
number: 16
title: sluice gives a place its ports and releases what its checks hand off
status: accepted
date: 2026-10-13
decision-makers:
- Chuck Swanberg
---

# sluice gives a place its ports and releases what its checks hand off

Deciders: the user, preparing sluice for a repository whose checks bring up a stack, 2026-10-13.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 31 to 34, 66, 83, 84, C.1, C.2; ADRs 0010, 0013; a survey of one repository's test stack (an AWS emulator in a container, stand-ins for the forge and two outside services, API and dev servers, Playwright), and research of 2026-10-13 comparing moon 2.6, devenv and Earthly for bringing such a stack up.

## Context and problem statement

ADR 0010 left a candidate's checks to bring up what they need, on ports they pick. A stack of separate systems, a container, stand-ins, servers and a browser test, finds its parts by port, so each part must know every other's before any starts; a free port picked by each part cannot give that. Several candidates are under test at once on one host, so ports taken from the branch's name, as worktrunk's `hash_port` gives a session, would be the same for every candidate of one branch. And what the checks hand to a container engine outlives the run's process group: once sluice ends a run at its time limit, the checks' own cleanup never runs, and a container left behind holds its port and fails the next candidate. What does sluice owe such checks?

## Considered options

1. Nothing more: the checks pick ports and clean up after themselves.
2. Ports from a hash of the candidate's name, and the checks clean up.
3. Ports from a block sluice holds for the place on the host, and a release command the policy names, run whenever sluice releases the place.

## Decision outcome

Option 3. `[checkout] ports` names the ports a candidate's stack uses. The place seam holds a block of ports for the place on the host, across repositories, with every port of the block checked free, and gives each name a fixed position in it: `ports_from + block × names + position`, `ports_from` 20000 by default, above `hash_port`'s range. The place carries `SLUICE_PLACE` and a `SLUICE_PORT_<NAME>` for each name into its runs. A block is never held by two places at once, so two candidates never collide, where a hash can; a name's port is the same for the place's whole life. (Amended 2026-11-13: what a place holds is its ports themselves, below.)

`[tests] release` names a command the check seam runs in the place, under the same executor, limits and environment as the checks, whenever the core releases the place: after a pass, a failure, a time limit, or a run found dead by a later round. It removes what the checks started outside their process group. It runs under the executor because it is the candidate's own code, as worktrunk's hooks were when ADR 0013 refused them.

`[executor] docker = true` lets a sandboxed run reach the container engine's socket. A container the run starts may mount any path, so such a run is recorded as not confined (84).

Option 1 cannot wire separate systems together. Option 2 collides by chance and still leaves containers behind after a time limit. moon's `cleanup` dependency is no substitute: it does not run after a signal, which is how a time limit ends a run.

### Consequences

- The statement's clauses 31 to 34 are rewritten and C.1 and C.2 bind them; rule core.10 is rewritten.
- Supersedes ADR 0010's "on ports they pick"; its decisions on services, the operator's word and the submit gate stand.
- A repository's stack is started by its own tools, devenv's processes with their readiness checks, from a moon task the checks depend on; sluice runs moon as before.
- The release command runs once per place release and may run again after a crash, so it must be safe to repeat.

## Amendment of 2026-11-13: a place holds its ports themselves, not a block's number

agentplot-design's, drawn from clauses 31 and 32 and this record's own guarantee that two places never collide. On chuck-herdr-alpha under sluice 0.93.2, one port, 20285, went to two live places at once, a run of switchboard-kit's #2024 and the place of #2025, and 19 of 190 release tests failed with no code at fault (signal 2026-11-13-swb-2-conductor-5fe85d76/01, wldn's). The host held block numbers, one numbering for every repository and branch, while each place reckoned its block's ports from its own policy's count of names. Policies that list different numbers of ports, two revisions of one repository's or two repositories', give overlapping ranges for different numbers: at 21 names block 7 is 20147 to 20167, and at 19 names block 8 is 20152 to 20170. The check that each port is free catches the overlap only once the other place's stack is listening.

- **A place holds its ports.** What the host records for a live place is each port it holds, and a new place takes the lowest run of as many ports as its policy names in which no port is held by a live place and each is free. Two live places never hold one port, whatever their policies list.
- **Fixed when the place is made.** A place's ports are recorded with the place when it is made, and every later reading of them, its runs', its release's and a status's, reads that record and never reckons them again from a policy. A policy's change takes effect for places made after it.
- Keeping a block numbering with one size for the whole host was the other path: every repository on the host would have to list ports within that size, and a policy listing more would be refused or overlap.
- Rule core.10 is amended to match.

## Amendment, 2026-11-13: what the build of a place's ports chose

Status: accepted on 2026-11-13 as the choices of the change `one-sluice-process-runs-each-hosts-queues`, its design D18 and its code stage, each within the amendment above and cheap to change.

Deciders: the design and the code stage of the change `one-sluice-process-runs-each-hosts-queues`.

Sources: the statement's clauses 31 and 32; rule core.10; the change's design D18; `crates/sluice/src/profiles/places.rs` (`hold_ports`, `free_ports`, `Made::ports`).

### Decisions

1. **Each port is held by the port.** The host holds a live place's ports at `$XDG_STATE_HOME/sluice/ports/held/<port>`, one file for each port, naming the record of the place that holds it, under `ports/.lock` as blocks were; a port whose file names a record that is gone is free again. A new place walks from `ports_from` and takes the first run of as many ports as its policy names in which no port is held by a live place and each is free, starting the walk again past the first port it finds held or busy. *Not taken:* one file for each place listing its ports, which a new place would read whole to learn what is free; a file of every held port, rewritten whole at each hold.
2. **The record is written before its holds.** A place's record is written first, so a hold names a record that exists and a stop between leaves a record the release reads; then each port is held; then the record is written again with `ports`, each `SLUICE_PORT_<NAME>` and its port, a candidate's place among them, which kept only its block's number before. Every reading of the place's ports, its runs', its release's, a gone place's stored release, a preview's names and a status, reads that record. *Not taken:* holding the ports first, which leaves a hold naming no record a stop part way would leave for good.
3. **An older sluice's blocks are read by nothing.** Its `ports/<n>` files stay under the same lock until removed by hand; a place it made, whose record names a `block`, is read with the field passed over, its ports read from its record's `ports` where it has them. The check that each port is free still passes by a port an older place's stack listens on. *Not taken:* reading an older place's block through its policy, a map from one shape to the other (principle 12).
4. **The release notes say to release worktrees' places first.** A worktree's place an older sluice made holds its ports by its block, which this build reads nothing of, so a host releases its worktrees' places with `sluice release` before installing the release that carries this. *Not taken:* sweeping older blocks into holds at the first start, which would write a hold for a place this sluice did not make.
