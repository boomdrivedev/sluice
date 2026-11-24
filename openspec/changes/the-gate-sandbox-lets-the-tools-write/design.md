# Design

## Context

See proposal.md, Why. The code as it stands:

- `Sluice::after_run` (`crates/sluice-core/src/step.rs`) reads a run in
  which a check did not pass. It asks the check seam, in order, whether
  the host's system was switched, what the executor refused the run,
  and whether the container engine has gone. The first that answers
  gives the reason: every check that failed, was failed by a task it
  depends on, or was left without a result has no outcome. Nothing is
  recorded, nobody is blamed, and the combination runs again, up to
  the runs in a row the deployment states (115).
- The sandbox's profile (`sandbox_profile` in
  `crates/sluice/src/profiles/checks.rs`) lets a run write its checkout,
  its scratch, the host's `[[caches]]` and moon's store. Under
  `network = "localhost"` it refuses every connection but localhost's,
  name lookups among them.
- `devenv.nix` links each checkout's `.moon/cache/outputs` to
  `moon-outputs` in the git common directory, so every worktree shares
  moon's task archives.

## Measurements

On deneb, 2026-11-24, `sluice exec main -- moon run cli:test` with the
kernel's sandbox reports streamed beside it. Every run of `cli:test`
was refused the same things, whether its tests passed or failed:

| Program | Refused | Why it does it |
|---|---|---|
| moon | `network-outbound remote:*:53`, the resolver's socket | proto's check of whether it is offline, at the start of a task with a toolchain |
| moon | `file-write-create <git common dir>/moon-outputs/<hash>.tar.gz` | the task's archive, through the checkout's link |
| cargo | `file-write-data ~/.cargo/.package-cache`, `.package-cache-mutate`, `.global-cache`; `file-write-xattr` on `~/.cargo/registry` and `~/.cargo/git` | its lock files and its record of what it used |

All 949 tests passed in that run. moon's offline check with
`PROTO_OFFLINE=1` made no request (the same task, run again under the
same sandbox). In #160's log, runs 0 and 1 of the combination each lost
`cli:test` to those refusals, and run 2 passed: 17.5 minutes for a gate
whose slowest task takes 9.

## Decisions

### D1. A fail stands, whatever the sandbox refused

A check that ran and failed is a fail, recorded under its key and
blamed. `after_run` still reads what the sandbox refused, after a run
in which a check did not pass, from the sandbox's own report. It keeps
the words on the run's report, `RunReport::refused`, and they are said
beside the fails: on the run's `detail` line in the branch's log, in
`sluice status` and in `sluice exec`. A switch of the host's system,
or an engine gone, still gives the run's checks no outcome (84; records
0074, 0097): there the host changed under the run.

Before: a check that did not pass in a run whose sandbox reported any
refusal had no outcome. The combination ran again.

Now: the fail is recorded and blamed. The refusal is said beside it.

Why: every moon run with a toolchain is refused a name lookup, so
every fail was erased, and a real failure cost two runs more and lost
its outcome. A refusal that makes a test fail is a fault at its cause:
the branch's policy, the host's `[[caches]]` or the test, fixed there
(rule core.37 as amended 2026-11-23; the decision never to set aside).
A fail under one grant is never weighed with a pass under another
(138; rule core.34), so a wider sandbox later reads afresh.

Proposed in Boom as `a-fail-stands-whatever-the-sandbox-refused`, which
changes `a-refusal-is-read-from-the-executors-own-report`.

### D2. A run held from the network is told it is offline

Where the run is sandboxed and its policy's network is `denied` or
`localhost`, the run's command starts with `PROTO_OFFLINE=1`. proto
reads it, and moon through proto, as offline, and asks nothing. The
equipping of the checkout, outside the sandbox, may need the network
(proto's install) and gets nothing. sluice names proto in the check
profile only (rule core.1).

Option not taken: allowing name lookups under `localhost`. The policy
says the run reaches localhost alone; a lookup reaches the network.

### D3. The sandbox opens moon's task archives

moon 2.6.1 writes each task's archive to the checkout's own
`.moon/cache/outputs`, whatever its shared worktree setting. Under the
moon runner, the sandbox opens that folder where it resolves, as it
opens moon's store (record 0105): a link out of the checkout is opened
where it leads, and a folder in the checkout adds nothing. Each is a
`<cache>` in the grant, so the grant of a run whose archives are in its
checkout is unchanged (138).

### D4. cargo's home is the host's to declare

cargo's writes to its home fail, and cargo goes on: no test fails for
them. sluice names no tool the repository uses outside its runner, so
the sandbox opens no folder of cargo's of its own. A host that wants
cargo's locks and its record of use kept declares the folder one of its
`[[caches]]` (35). On deneb that is swancloud's `lib/host-caches.nix`.

### D5. The flaky tests

- `remote_branch a_held_landing_lands_at_the_next_round_once_the_forge_takes_it`
  and `a_held_landing_refused_with_other_words_is_held_with_them` step
  once and read the state. The host process's own round, after the
  submit, may hold `main`'s lock, and the step then answers `busy:
  another round holds the lock` and runs no round: 6 fails in 12 runs.
  Each steps until a step runs a round: 11 passes in 11 runs.
- `host_process s75_a_combination_starts_before_a_third_agents_check`
  fails at every run under load: its agents' checks sleep 3 and 9
  seconds, and the combination must be waiting for room before the
  first ends. Each agent's check now holds until the test lets it end,
  once the overview shows the combination and the third agent waiting
  for room.
- `host_process a_newer_client_has_an_older_host_process_hand_over`
  and `an_upgrade_keeps_its_runs_as_children` read the host process's
  pid after a hand-over.
