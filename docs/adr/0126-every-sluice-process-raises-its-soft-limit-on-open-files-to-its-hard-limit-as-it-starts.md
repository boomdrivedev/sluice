---
number: 126
title: Every sluice process raises its soft limit on open files to its hard limit as it starts
status: accepted
date: 2026-11-15
decision-makers:
- the brief of the chore sluice-raises-its-open-files-limit-at-start
- the code stage of chore/sluice-raises-its-open-files-limit-at-start
---

# Every sluice process raises its soft limit on open files to its hard limit as it starts

Status: accepted. The brief decides what sluice does, and the statement's 135 as amended 2026-11-15 says it. How it does it is the code stage's choice, each part cheap to change.

Deciders: the brief of the chore `sluice-raises-its-open-files-limit-at-start`, on the bolt `bolt/sluice-runs-under-any-hosts-file-limit`: "sluice raises its soft limit on open files to its hard limit when it starts, so it and the moon runs it starts are not held to a supervisor's low default such as launchd's 256". The code stage of `chore/sluice-raises-its-open-files-limit-at-start` chose the form below.

Sources: deneb on 2026-11-15, where `launchctl limit maxfiles` answers `256 unlimited` and `sysctl kern.maxfilesperproc` answers `245760`; the rlimit crate 0.11.0, `increase_nofile_limit` in `src/tools.rs`; Go's runtime, which since Go 1.19 raises its own soft limit to its hard limit and gives the processes it starts the limit it started with; the statement in boomdrivedev/blueprints, `design/sluice/requirements.md`, clause 135 as amended on the branch `design/sluice-raises-its-open-files-limit-at-start` (2a045b2), and principle 14 as clause 151 cites it. The family's tools at their origin/main of 2026-11-15, flume, weir, boomdrive, boom, timber and crew, set no limit on open files, and no host configuration of theirs raises one.

## Context and problem statement

A process inherits its soft limit on open files from what started it. launchd gives what it starts a soft limit of 256. A sluice started under launchd, or under anything launchd started, runs its rounds, its git reads and the checks it starts under 256 open files. A moon run opens more than that: its cache, its task outputs, each process it starts and their pipes. A run that reaches the limit fails with "Too many open files", and the fail reads as the candidate's.

The soft limit is the process's own to raise, up to its hard limit, with no privilege. The host could raise it instead, for every supervisor that could start sluice. But 135 says nothing is installed for sluice and no host sets its environment for it (151, principle 14).

## Considered options

1. **sluice raises its own soft limit to its hard limit as it starts, and the runs it starts inherit it** (chosen).
2. **The host raises it.** A launchd plist's `SoftResourceLimits`, `launchctl limit maxfiles`, a systemd unit's `LimitNOFILE` or a shell's `ulimit`, in each place that starts sluice. Every supervisor needs its own setting, one left out holds sluice to its default again, and 135 rules out a host configuring itself for sluice.
3. **sluice raises its own, and hands its runs the limit it started with**, as Go's runtime does. The moon runs are what open the most files, and the brief names them.
4. **sluice raises the limit of each run it starts, and not its own.** sluice's own process, the host process above all, holds a repository's pack files, its records and its children's pipes. One raise as it starts covers sluice and every process it starts, the supervisor and the checks included.
5. **A number of sluice's own, 10240 say.** The hard limit is the host's own bound; a number of sluice's is one more a host might need to change.
6. **sluice's own code through nix's `resource` feature, with a sysctl call of its own for macOS's cap.** The rlimit crate's `increase_nofile_limit` already reads `kern.maxfilesperproc` and sets no more than it, which macOS's `setrlimit` refuses above. sluice takes a maintained crate, as it does for its backoff (core.29).

## Decision outcome

- **First, before anything else.** `main` raises the limit right after it reads what a `sluice run` handed it, before it parses its command line, reads a file or starts a thread or a process. Every sluice process raises its own: a command, the host process, a run started afresh, a supervisor and the hook.
- **To the hard limit, as far as the system lets one process open.** `rlimit::increase_nofile_limit(u64::MAX)` sets the soft limit to the hard limit, and on macOS to no more than `kern.maxfilesperproc`. A soft limit already at the hard limit is left as it is.
- **The runs inherit it.** Nothing of sluice's sets a run's limit, so a check, its moon and everything it starts have the raised limit.
- **A refusal is said and sluice goes on.** Where the system refuses the raise, sluice says `sluice: warning: the limit on open files stays as sluice was started with: <the system's words>` on its standard error and goes on under the limit it was given, as it ran before this record.

### Consequences

- No host raises launchd's limit, or any supervisor's, for sluice.
- A check that needs a lower limit sets its own in its command, `ulimit -n` say. A program that closes every descriptor up to its soft limit as it starts walks the raised limit, up to 245760 on deneb.
- Given a soft limit above `kern.maxfilesperproc` and an unlimited hard limit, as a shell on deneb has, the crate sets the soft limit to `kern.maxfilesperproc`. That is lower in name only: macOS lets no process open more than `kern.maxfilesperproc`, whatever its soft limit says.
- On Linux an unlimited hard limit is refused above `fs.nr_open`. sluice then warns and runs under the limit it was given.
- The exec test `a_command_runs_under_the_raised_limit_on_open_files` starts sluice under a soft limit of 256 and reads the limit its command runs under. Rule core.59 holds the decision.
