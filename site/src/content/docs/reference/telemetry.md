---
title: Telemetry
description: What sluice sends over OTLP, what a host sets up to receive it.
sidebar:
  order: 4
---

sluice sends; the host decides where it goes. Nothing reads telemetry
back: the queue never depends on it
([ADR 0014](/sluice/decisions/0014/)).

## What sluice sends

- **Off unless asked**: the host's
  [`[telemetry]`](/sluice/reference/configuration/#telemetry), in
  `~/.config/sluice/config.toml`, turns it on with `send = true`. A
  command's own `--otel`, or `SLUICE_OTEL=1` (or `true`, `yes`, `on`),
  turns it on for that command alone, and `--otel=false` or
  `SLUICE_OTEL=0` turns it off for that command, whatever the table
  says.
- **Off again**, by the table or by OpenTelemetry's own settings. The
  table's `traces = false`, `metrics = false` or `logs = "off"` turns
  that signal off, and no variable turns it on again.
  `OTEL_SDK_DISABLED=true` leaves telemetry off as if it were never
  asked for: nothing is sent, and a run's checks get nothing of it.
  `OTEL_TRACES_EXPORTER=none` turns traces alone off: metrics and logs
  still go, and a run's checks get the setting and no `TRACEPARENT`.
  `OTEL_METRICS_EXPORTER=none` and `OTEL_LOGS_EXPORTER=none` do the same
  for theirs. A setting only turns a signal off: any other value, `otlp`
  among them, leaves it as the table has it.
- **Where**: OTLP over plain HTTP, protobuf, to the table's `collector`,
  at its `/v1/traces`, `/v1/metrics` and `/v1/logs`. A table that names
  one takes none of the caller's `OTEL_EXPORTER_OTLP_*`: an agent's
  session that sets them for its own tools never sends sluice's
  telemetry elsewhere, or with its headers. sluice refuses a table with
  `send = true` and no `collector`. With no table, or where a command's
  own `--otel` sends beside a table that names no `collector`,
  `OTEL_EXPORTER_OTLP_ENDPOINT` names it, by default
  `http://localhost:4318`. No TLS: something on the host receives it and
  forwards it.
- **As**: service `sluice`, or the table's `service`, with
  `service.version` the release's and `host.name` the host's own name.
  The table's `attributes` add the host's own, e.g.
  `"deployment.environment" = "prod"`, and a `host.name` there names the
  host in sluice's place. A command's `--otel-service-name` or
  `SLUICE_OTEL_SERVICE_NAME` names the service for that command. With no
  table, the standard `OTEL_RESOURCE_ATTRIBUTES` adds the host's own
  instead; with one, sluice reads it not at all.
- **Traces**: one per round of a branch, `sluice.round`, with
  `sluice.recover`, `sluice.decide` and `sluice.perform` beneath it, and
  `sluice.form`, `sluice.start`, `sluice.verdict`, `sluice.blame` and
  `sluice.land` beneath those. Attributes: `sluice.branch`,
  `sluice.submissions`, `sluice.candidate` (the revision tested),
  `sluice.executor`, `sluice.confined`, `sluice.outcome`, `sluice.ran`,
  `sluice.replayed`.
- **A command inside its caller's trace**: a command started with a W3C
  trace context in `TRACEPARENT`, and `TRACESTATE` where given, as an
  agent program hands the commands it runs, sends one span in that
  trace while traces go, beneath the caller's span. It is named by the
  command's words as `sluice --help` lists them, `sluice submit`,
  `sluice exec` or `sluice` for the home view, and holds `sluice.exit`,
  the command's exit. A command started under none sends no span of its
  own. A round stays a trace of its own whatever command waits on it,
  and sluice's host process keeps no caller's context.
- **A submission keeps its caller's trace**: its record holds the
  `TRACEPARENT` its submit was started under as `trace`, whether or not
  the submit sends telemetry. Each `sluice.start` and `sluice.verdict`
  span of a combination that holds it links to that context, the link
  holding `sluice.submission`, so a backend leads from an agent's wait
  to the rounds that tested its work. A pull request's submission keeps
  none.
- **An exec's span**: `sluice exec` started under a caller's context
  holds `sluice.branch`, `sluice.ran` and `sluice.replayed`, and beneath
  it a `sluice.check` span for each check its run ran, over the times
  its runner gave, with `sluice.task` and `sluice.outcome`, `pass` or
  `fail`. A check answered from an earlier run has none. A command after
  `--` is its run's one check, named by its words with whatever reads as
  a secret `[redacted]`. The run's checks get `TRACEPARENT` naming the
  exec's span, as a round's run names its `sluice.start`.
- **A caller's trace turns nothing on**: it says only where sluice's
  spans nest. Whether sluice sends is the command's own choice, else the
  table, as above.
- **A submission's life**: while traces go, the write that ends a
  submission sends one span, `sluice.submission`, from its submission to
  that end: landed, `refused`, blamed (`failed`), dropped for a conflict
  (`conflict`) or `withdrawn`, whichever host or command wrote it. It is
  in the trace the submission was submitted under, beneath the caller's
  span its record kept, or the root of a trace of its own where it kept
  none, and it links to the span of the round or command that ended it.
  It holds `sluice.repository`, `sluice.branch`, `sluice.submission` (its
  number), `sluice.head`, `sluice.work`, `sluice.agent`,
  `sluice.outcome`, its seconds waiting with no combination of it under
  test, `sluice.waited_seconds`, and under test, `sluice.tested_seconds`,
  and `sluice.no_outcome`, the runs of its combinations that decided
  nothing. Only a blame is an error: a run with no outcome is named, and
  is never the submission's failure. A submission's record keeps its
  time under test, `under_test_ms` and `under_test_since`, and
  `runs_with_no_outcome`, whether or not telemetry is on; nothing sluice
  decides reads them.
- **Metrics**, each with `sluice.repository` and `sluice.branch`, and
  `sluice.outcome` where the table names it, and nothing else, so no
  metric carries a value that grows with each run. `sluice.repository`
  is the owner and name of the repository's `origin`, `acme/app` for
  `git@github.com:acme/app.git` or `https://github.com/acme/app`, else
  the repository's folder's name, `.git` dropped; nothing else of the
  origin's address is kept:

  | Metric | Kind | What |
  |---|---|---|
  | `sluice.queue.depth` | gauge | submissions not yet out of the queue |
  | `sluice.window` | gauge | combinations the branch tests at once |
  | `sluice.verdicts` | counter | results read, by `sluice.outcome` |
  | `sluice.tasks.ran`, `sluice.tasks.replayed` | counter | tasks run, and answered from the cache |
  | `sluice.tasks.time_saved` | counter, seconds, declared in `s` | the time each replayed task's key last took when it ran |
  | `sluice.culprits` | counter | submissions blamed for a failure |
  | `sluice.conflicts` | counter | submissions removed for a conflict |
  | `sluice.landed`, `sluice.refused` | counter | submissions landed; landings refused because the branch moved |
  | `sluice.submission.landed_after` | histogram, seconds, declared in `s` | submit to landed |
  | `sluice.candidate.tested_for` | histogram, seconds, declared in `s` | a combination's time under test, by `sluice.outcome` |
  | `sluice.submissions.ended` | counter | submissions ended, by `sluice.outcome`: `landed`, `refused`, `failed`, `conflict` or `withdrawn` |
  | `sluice.submission.waited` | histogram, seconds, declared in `s` | a submission's time waiting with no combination of it under test, by `sluice.outcome` |
  | `sluice.submission.tested` | histogram, seconds, declared in `s` | a submission's time under test, combinations that overlap counted once, by `sluice.outcome` |
  | `sluice.host.running` | gauge | the host's checks running for the branch, sent by sluice's host process every three seconds, and 0 once after its last run |
  | `sluice.host.bound` | gauge | the host's bound, the same on each branch's row: read `sluice.host.running` summed over branches against its greatest |
  | `sluice.run.waited_for_room` | histogram, seconds, declared in `s` | how long a run waited for room under the host's bound, from its ask first refused to its hold |

- **Logs**: a failed check's output, and sluice's own warnings and
  errors, under the span they were emitted in. The table's `logs`, or a
  command's `--otel-logs`, says how much of a failure goes. `off` sends
  none, and `summary` sends which task failed and where its output is
  kept. `tail` sends the output's end, scrubbed of what reads as a
  secret, at most `logs_tail_bytes` of it.
- **The checks inside a run**: with telemetry on, a run gets the
  collector sluice sends to and the traces and logs it sends, and no
  metrics: its environment holds `OTEL_METRICS_EXPORTER=none`, with a
  table or without, so a run's tools send their spans and logs, which
  name the round that started them, and no metric names a run. moon runs
  with `--otel`, so its own traces arrive under the service it names
  itself. With a `[telemetry]`, a run gets the table's collector, or the
  caller's where the table names none, and none of the caller's other
  `OTEL_*`. Each other signal sluice does not send goes to it as
  `OTEL_<SIGNAL>_EXPORTER=none`. With no table, a run gets sluice's
  `OTEL_*` as they are, but for metrics. moon does not nest its spans
  under sluice's: each carries `host.name`, the table's attributes,
  `sluice.trace_id`, `sluice.span_id`, `sluice.place` and
  `sluice.revision` as resource attributes, so a search for the round's
  trace id finds them.

## What a host needs

1. **A receiver on `localhost:4318`**, OTLP/HTTP. It must listen on
   localhost: sluice's sandboxed runs reach localhost and nothing else.
2. **A [`[telemetry]`](/sluice/reference/configuration/#telemetry)
   table** in the host's own settings, `~/.config/sluice/config.toml`,
   that sends to it and names what the host adds:

   ```toml
   [telemetry]
   send = true
   collector = "http://localhost:4318"

   [telemetry.attributes]
   "deployment.environment" = "prod"
   ```

   On nix-darwin and NixOS, sluice's module writes it from
   `services.sluice.settings.telemetry`. Nothing goes in a session's
   environment, and nothing wraps sluice: every command on the host
   reads the table, an agent's `sluice submit` and sluice's host process
   alike. The host process takes a change of the table up by handing its
   repositories to a fresh host process it starts itself, with nothing to
   restart.
   sluice names the host itself, so hosts sending to one backend stay
   apart.
3. **No secrets in sluice's settings.** An API key belongs to the
   receiver's own configuration. sluice's file names no headers, and a
   module may write it where every user can read it.

When nothing is listening, sluice loses its telemetry and carries on: a
refused connection costs nothing. A receiver that accepts and never
answers delays each `sluice step`'s exit by up to the exporter's
timeout, 10 seconds.

## Receivers

A viewer on a developer's machine, keeping nothing once it is closed;
one of these at a time, since both take port 4318:

```sh
nix run nixpkgs#otel-tui              # in a terminal
nix run nixpkgs#otel-desktop-viewer   # in a browser
```

Any OpenTelemetry collector, or an agent with an OTLP receiver on
localhost, forwards to a backend over TLS itself. The repository's
[docs/telemetry.md](https://github.com/boomdrivedev/sluice/blob/main/docs/telemetry.md)
has the receiver configurations.
