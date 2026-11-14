## Why

Record 0093, accepted by the user on 2026-11-06 ("I approved 93.") and
amended on their card of 2026-11-08, gives a host one sluice process for
its queues, with no intake mark, no hand-over between hosts and no wait
that runs a round. The statement carries it in clauses 135, 129, 71, 150
and 137 (boomdrivedev/blueprints, `design/sluice/requirements.md`; 71 at
f59c02f and 129 at 862b041 until the user pushes them), and rules
core.31, core.25, core.33 and core.49 bind it. Four later decisions ride
on the same process: a freed part of the host's bound goes to the
repository with the fewest runs under test (71; record 0123); every read
of a queue follows the host process and a result is read once, by key,
and never taken away with its branch (129, 69, 109; record 0093 as
amended 2026-11-13); a place holds its ports themselves (31, 32; record
0016 as amended 2026-11-13; core.10); and the host process takes up a
changed host settings file as a kept `sluice run` does (135; record
0107; core.54).

`bolt/sluice-runs-one-host-process` built most of this at e61acba, and
`fix/sluice-runs-one-host-process--passes-mains-checks-954d21e` at
47ef4b8a, but main moved more than five hundred commits past them, and
what main holds at 7ce112b4 is what the record removes:

- `sluice run` (`Command::Run`, `drive`, `Driving` in
  `crates/sluice/src/lib.rs`) is one repository's driver under
  `readers/<repository>.driver`. Where none lives, every `sluice wait`,
  waiting `sluice submit` and `wt submit` runs rounds between its reads
  (`wait_on`), so ten waits are ten processes forming one branch's
  combination.
- The host's bound is counted across processes through `running/`
  (`host_count.rs`). A round counts its room when it decides and admits
  the run only at its start, so an agent's `sluice exec` takes the room
  between the two, and nothing shares a freed run among repositories:
  on 2026-11-13 switchboard-kit held all three of a box's runs while
  willdan-apps-portal's proven bolt waited hours for one (record 0123).
- A branch whose records each host keeps is taken in by whichever host
  holds `refs/sluice/intake/<branch>` (`pulls.rs`, `pushed_refs.rs`,
  `kept.rs`). Hosts hand requests over by withdrawing them, and a request
  comes back under a new number each time: willdan-apps-portal's #1 was
  submissions 358 to 363 on 2026-11-13 with nothing tested.
- A one-shot `sluice status`, `sluice overview` or forecast with no
  reader fetches the repository under its lock and loads the whole
  results log: on 2026-11-13 each overview of switchboard-kit fetched
  GitHub and read 60 MB, and waits missed their 10-second bound.
- A place takes a block number, `ports/<n>`, and reckons its ports from
  its own policy's count of names, so two policies of different lengths
  give overlapping ports: port 20285 went to two live places on
  2026-11-13 and 19 of 190 tests failed.
- `sluice run` takes up a changed settings file between passes and
  starts itself afresh for `[telemetry]`; nothing else that keeps
  running does.

## What Changes

- **sluice's host process.** A hidden `sluice host` is the one process
  for a user on a host. A command that needs the host's queues moved
  reaches it over a socket in the user's own short directory, or starts
  it detached where none answers; ten commands at once start one. It
  holds a task for each repository with work, each apart, finds its
  repositories again from an index in the host's state, keeps nothing
  the records do not hold, and ends once no queue has work in play and
  nothing follows it. A command of a newer sluice has an older host
  process hand over and end.
- **Waits and watches follow it and run no round.** `sluice wait`, a
  waiting submit, `wt submit` and `sluice overview --watch` follow its
  document; a submit writes its entry, tells it, and returns or waits.
  `sluice step` asks it for one round. `sluice run` is hidden and
  follows its passes.
- **The host's room in one place.** A combination's run holds its part
  of the bound from the count that chose it to its start. A
  combination's run starts before a session's or a preview's, taking
  turns with them, and sessions' and previews' runs start in the order
  they came to wait (150). A part that frees goes to the repository with
  a run ready and the fewest runs under test, the longest waiting first
  among equals; nothing under test is stopped, and a pool's runs are
  outside the bound (71; record 0123).
- **Every read follows the host process.** A status, an overview, the
  home view and a forecast ask the host process for a reading current
  within its tick, starting it where none answers, and then read the
  records as the host holds them, fetching nothing of their own. The
  host process reads each recorded result and signed run record once,
  verifying each signature once, and answers by key, and a command that
  needs results asks it for them. A branch that lands or is removed
  keeps every result it recorded (129, 69, 109).
- **A place holds its ports.** The host holds each port a live place
  holds, by the port; a new place takes the lowest run of as many ports
  as its policy names in which no port is held and each is free; the
  ports are recorded with the place and read from that record ever
  after (31, 32; record 0016 as amended).
- **The host process takes up changed settings.** Between its passes
  it looks at the host's settings file as a kept `sluice run` does,
  taking a change up in place at each repository's next pass. A change
  it cannot take up in place, `[telemetry]`'s, it takes by handing its
  repositories to a fresh host process it starts itself, so work in play
  never waits for another command; nothing passes between the two but
  what the records and the host's state hold, and runs already started
  keep the settings they started under (135; record 0107 as amended
  2026-11-13; core.54).
- **No intake mark, no hand-over.** The instance that keeps a branch on
  its remote takes its requests in, and no other; nothing pushes, renews,
  leases, reads or fetches an intake mark, and nothing reads one an
  older sluice left. No withdrawal is written for a hand-over, so a
  request keeps one number from intake to landing; one an older sluice
  wrote is read as written.
- **BREAKING: a branch whose records each host keeps takes no pull
  requests.** A `pull_requests` table on a branch whose policy names no
  `[signers]` refuses that branch alone, naming `[signers]`, as a word
  sluice no longer keeps is (56, S64, 137). Every other branch reads as
  before.
- **Intake says what it met, once.** `sluice step` prints every warning
  its round's intake met. A standing intake failure is logged on the
  branch once for each cause and host with its end, kept beside the
  round's refusal of the remote, and shown by `sluice status` (128, 133;
  core.28).
- **The docs stop naming a service**, and say what changed: the README,
  the site's pages and the plugin's skills.

## Capabilities

### New Capabilities

- `host-process`: sluice's one host process for a user, how commands
  reach and start it, what it holds, how it finds its repositories,
  when it ends, how a newer sluice takes over, and how every read
  follows it (135, 129).
- `place-ports`: a place holds its ports themselves, fixed when it is
  made (31, 32; record 0016).

### Modified Capabilities

- `host-driver`: `sluice run` drives nothing; it follows the host
  process, and the per-repository driver's requirements go.
- `overview-reader`: the reader is the host process; a one-shot read is
  answered from its last reading and fetches nothing (129).
- `submission-wait`: a wait follows the host process and runs no round;
  a landing stands whatever its round failed after; a wait across hosts
  names the host that keeps the branch.
- `reader-tick`: the idle tick is the host process's.
- `forecast`: a forecast with no current reader has the host process
  read rather than fetching itself.
- `run-bound`: the count, each run's part from its count to its start,
  the turns, the waits and the share among repositories are kept in the
  host process (71, 150; core.49).
- `standing-executor`: the host process claims the runs its signer
  counts for.
- `run-signing`: results are read once and by key, and a branch that
  lands or is removed keeps every result it recorded (69, 109).
- `settings-take-up`: the host process takes up changed settings, and
  hands its repositories to a fresh host process for a change it cannot
  take up in place; a kept `sluice run` follows.
- `request-intake-host`: the instance that keeps the branch on its
  remote takes its requests in; every mark, lease, hold and hand-over
  requirement goes.
- `pull-request-intake`: a pull request table on records each host
  keeps refuses its branch alone; a standing intake failure is said once
  for each cause and host.
- `remote-read-log`: intake's refusal of the forge is logged once per
  cause and host with its end, and shown by status.
- `overview`: a branch another host keeps lists the requests this host
  follows, naming the keeper.
- `obsolete-combinations`: a hand-over is no longer among what makes a
  combination obsolete.

## Impact

- **Code.** A new module `crates/sluice/src/host_process/` (lifecycle,
  socket, protocol, client, environment, index, tasks, pass, room and
  the results index), ported from e61acba and 47ef4b8a onto main's code.
  `crates/sluice/src/lib.rs` (run, step, wait, submit, status, overview),
  `main.rs`, `exec.rs`, `preview.rs`, `reader.rs`, `host_count.rs`,
  `hand_over.rs`, `home.rs`, `overview.rs`, `profiles/checks.rs`,
  `profiles/places.rs`, `profiles/records.rs`, `profiles/local_files.rs`,
  `profiles/remote_refs.rs`, `profiles/pushed_refs.rs`,
  `profiles/github/{pulls,kept}.rs`; `crates/sluice-seams` (`Part`,
  `RunKind`, `Tests::hold`, `State::results_for`); `crates/sluice-core`
  (`step.rs` starts holding parts, the policy's refusal, intake's
  refusal, no hand-over); the fakes.
- **Tests.** A new target `crates/sluice/tests/host_process.rs` on real
  git; `cli.rs`, `reader_tick.rs`, `checks.rs`, `instance.rs`,
  `pull_requests.rs`, `pushed_refs.rs`, `peers.rs`, `remote_branch.rs`
  and `places.rs` keep their commands and read rounds from the host
  process.
- **Records.** sluice's `sluice/log@1` `read-refused` and `read-again`
  lines gain an optional `intake`; a place's record names its ports;
  `withdrawn.handed_over_to` is read and never written.
- **Docs.** README, the site's commands, state, configuration,
  walkthrough, overview, executors, pull requests and telemetry pages,
  the plugin's skills; records 0093, 0016, 0107 and 0123 amended by the
  build's choices, and a record of the intake choices; rules core.4,
  core.10, core.25, core.28, core.31, core.33, core.49 and core.54's
  details.
- **Hosts.** Nothing to install. A host's `sluice run` units keep
  running harmlessly as followers until removed.
- **Told to the orchestrator.** sluice's own `main` and
  WilldanGroup/switchboard-kit's `main` take pull requests with no
  `[signers]`, so each is refused on the release that carries this until
  its policy names signers whose keys its hosts hold; which hosts sign is
  ops' choice and not this change's. crew reads `taken_in_by` from a
  wait's document and loses its `holder`.

## Out of scope

- **A queue's home written in its records, one lead while a run is live,
  and a machine whose signature a branch does not accept leading none of
  its rounds**: the change `a-queues-home-is-written-in-its-records`,
  after this one (record 0122; 58, 72, 137).
- **A run's life written before it acts, and GitHub Actions running a
  queue's checks**: `a-runs-life-is-written-first-and-actions-runs-its-checks`
  (record 0122; record 0119). The refusal of a pull request table on
  records each host keeps names the forge's workflow once that change, or
  `a-github-action-runs-a-queue-whole`, builds a workflow a command
  writes; until then it names `[signers]` alone (core.26).
- **`sluice daemon`**, the host process kept running when idle:
  `sluice-runs-as-a-daemon`.
- **A preview asked for with no wait keeping its place**:
  `a-preview-asked-for-without-a-wait-keeps-its-place`.
- **One forge client for the host.** Each repository's task keeps its
  own; the allowance's pause is already one, in the host's state (134;
  core.29).
- **What other repositories owe**: Boomdrive's core.13 and record 0003,
  flume's C.6 step, and the hosts' per-repository `sluice run` units,
  each changed when its host moves to the release (principle 11).
