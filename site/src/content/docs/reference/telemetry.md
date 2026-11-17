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
- **Metrics**, each with `sluice.branch`:

  | Metric | Kind | What |
  |---|---|---|
  | `sluice.queue.depth` | gauge | submissions not yet out of the queue |
  | `sluice.window` | gauge | combinations the branch tests at once |
  | `sluice.verdicts` | counter | results read, by `sluice.outcome` |
  | `sluice.tasks.ran`, `sluice.tasks.replayed` | counter | tasks run, and answered from the cache |
  | `sluice.culprits` | counter | submissions blamed for a failure |
  | `sluice.conflicts` | counter | submissions removed for a conflict |
  | `sluice.landed`, `sluice.refused` | counter | submissions landed; landings refused because the branch moved |
  | `sluice.submission.landed_after` | histogram, seconds | submit to landed |
  | `sluice.candidate.tested_for` | histogram, seconds | a combination's time under test, by `sluice.outcome` |

- **Logs**: a failed check's output, and sluice's own warnings and
  errors, under the span they were emitted in. The table's `logs`, or a
  command's `--otel-logs`, says how much of a failure goes. `off` sends
  none, and `summary` sends which task failed and where its output is
  kept. `tail` sends the output's end, scrubbed of what reads as a
  secret, at most `logs_tail_bytes` of it.
- **The checks inside a run**: with telemetry on, a run gets the
  collector sluice sends to and the signals it sends, and moon runs with
  `--otel`, so moon's own traces and metrics arrive as service `moon`.
  With a `[telemetry]`, a run gets the table's collector, or the
  caller's where the table names none, and none of the caller's other
  `OTEL_*`. Each signal sluice does not send goes to it as
  `OTEL_<SIGNAL>_EXPORTER=none`. With no table, a run gets sluice's
  `OTEL_*` as they are. moon does
  not nest its spans under sluice's: each carries `host.name`, the
  table's attributes, `sluice.trace_id`, `sluice.span_id`,
  `sluice.place` and `sluice.revision` as resource attributes, so a
  search for the round's trace id finds them.

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
[docs/telemetry.md](https://github.com/agentplot/sluice/blob/main/docs/telemetry.md)
has the receiver configurations.
