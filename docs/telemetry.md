# Receiving sluice's telemetry: what a host sets up

A brief for the repositories that set up the hosts sluice runs on (crew's
EC2 host, swancloud and its crews, a developer's Mac). sluice sends; the
host decides where it goes. Nothing here changes sluice. The decision is
[ADR 0014](adr/0014-telemetry-is-opt-in-otlp-and-decides-nothing.md),
for logs [ADR 0022](adr/0022-telemetry-sends-a-failures-output-scrubbed-and-only-as-asked.md),
and for where it goes, the host's own `[telemetry]`,
[record 0106](adr/0106-sluices-telemetry-is-a-setting-of-the-hosts-own-settings-file.md)
(the statement's clause 151; rule core.53).

## What sluice sends

- **Off unless asked**: the host's `[telemetry]`, in
  `~/.config/sluice/config.toml`, turns it on with `send = true`
  ([configuration](../site/src/content/docs/reference/configuration.md#telemetry)).
  A command's own `--otel`, or `SLUICE_OTEL=1` (or `true`, `yes`, `on`),
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
  `"deployment.environment" = "crew"`, and a `host.name` there names the
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
  | `sluice.window` | gauge | candidates the branch tests at once |
  | `sluice.verdicts` | counter | verdicts read, by `sluice.outcome` |
  | `sluice.tasks.ran`, `sluice.tasks.replayed` | counter | tasks run, and answered from the cache |
  | `sluice.tasks.time_saved` | counter, seconds, declared in `s` | the time each replayed task's key last took when it ran |
  | `sluice.culprits` | counter | submissions blamed for a failure |
  | `sluice.conflicts` | counter | submissions removed for a conflict |
  | `sluice.landed`, `sluice.refused` | counter | submissions landed; landings refused because the branch moved |
  | `sluice.submission.landed_after` | histogram, seconds, declared in `s` | submit to landed |
  | `sluice.candidate.tested_for` | histogram, seconds, declared in `s` | a candidate's time under test, by `sluice.outcome` |

- **Logs**: one record for each failed task, sent by the round that
  reads its verdict, under that round's `sluice.verdict` span, with
  `sluice.branch`, `sluice.candidate`, `sluice.submissions`,
  `sluice.task`, `sluice.kept` (where its output is kept on the host),
  `sluice.bytes` and `sluice.replayed`; one for a preview whose command
  failed, with `sluice.preview`; and sluice's own warnings and errors.
  How much of a failure's output goes is the table's `logs`, or a
  command's `--otel-logs` or `SLUICE_OTEL_LOGS`:

  | Level | A failure's record carries |
  |---|---|
  | `off` | no logs are sent at all |
  | `summary` (default) | which task failed and where its output is kept, never the output |
  | `tail` | the end of the output, at most the table's `logs_tail_bytes`, or a command's `SLUICE_OTEL_LOGS_LIMIT`, in bytes (8192 by default, 65536 at most) |

  Every body sent is scrubbed first: the value of any variable in
  sluice's environment as the command started, the caller's
  `OTEL_EXPORTER_OTLP_HEADERS` among them, whose name holds `TOKEN`,
  `SECRET`, `KEY`, `PASS`, `AUTH`, `CREDENTIAL` or `HEADERS`, and
  anything shaped like a GitHub,
  AWS, Slack or Anthropic token, a JWT, a bearer token or a private key,
  becomes `[redacted]`. The output kept on disk is unchanged and stays
  the record; a log record lost on the way is never sent again.
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
2. **A `[telemetry]` table** in the host's own settings,
   `~/.config/sluice/config.toml`, that sends to it and names what the
   host adds, written where the host's configuration is managed: on
   nix-darwin and NixOS, sluice's module writes it from
   `services.sluice.settings.telemetry`.

   ```toml
   [telemetry]
   send = true
   collector = "http://localhost:4318"
   logs = "summary"

   [telemetry.attributes]
   "deployment.environment" = "crew"
   ```

   Nothing goes in a session's environment, and nothing wraps sluice:
   every command on the host reads the table, an agent's `sluice submit`
   or `wt submit` and a `sluice run` or `sluice step` alike. A `sluice
   run` kept running takes a change of the table up at its next pass by
   starting itself afresh, so a host keeps no restart of sluice for it.
   sluice names the host itself, `host.name`, so hosts sending to one
   backend can be told apart. Write the table once the host's sluice reads it: an older
   sluice refuses the file, as it refuses any table it does not know.
   Drop a wrapper that sets `SLUICE_OTEL*` or unsets the caller's OTLP
   variables in the same change.
3. **The table's `logs`**, if not `summary`:

   | Host | `logs` |
   |---|---|
   | a developer's Mac, to a local viewer | `"tail"` |
   | crew's EC2 host, to Datadog | `"tail"` only when everyone who reads the Datadog organisation may read the repository's check output; otherwise `"summary"` |
   | swancloud | `"summary"`, until its backend's access and retention are settled |

   Scrubbing lowers the chance a secret is sent; it does not rule it
   out.
4. **The table's `traces = false`**, for a host that keeps no traces:
   sluice, and moon inside its runs, then send metrics and logs alone.
5. **No secrets in sluice's settings.** An API key belongs to the
   receiver's own configuration. sluice's file names no headers, and a
   module may write it where every user can read it.

When nothing is listening, sluice loses its telemetry and carries on: a
refused connection costs nothing. A receiver that accepts and never
answers delays each `sluice step`'s exit by up to the exporter's
timeout, 10 seconds.

## Receivers

### A viewer on a developer's machine

Nothing kept once it is closed. One of these at a time, since both take
port 4318:

```sh
nix run nixpkgs#otel-tui              # in a terminal
nix run nixpkgs#otel-desktop-viewer   # in a browser
```

### The Datadog Agent (crew's EC2 host)

The agent's OTLP receiver, on localhost only, in `datadog.yaml`:

```yaml
otlp_config:
  receiver:
    protocols:
      http:
        endpoint: localhost:4318
```

or `DD_OTLP_CONFIG_RECEIVER_PROTOCOLS_HTTP_ENDPOINT=localhost:4318` in
the agent's environment. For sluice's logs, the agent's log collection
and its OTLP logs, also in `datadog.yaml`:

```yaml
logs_enabled: true
otlp_config:
  logs:
    enabled: true
```

or `DD_LOGS_ENABLED=true` and `DD_OTLP_CONFIG_LOGS_ENABLED=true`. At
`tail`, a Sensitive Data Scanner rule on service `sluice` is a second
line behind sluice's own scrubbing. The API key and site are the agent's
(`api_key`, `site`), from the host's secrets, never in a repository.
sluice's metrics are cumulative; the agent turns counters into rates.

### A hosted backend, through a collector (swancloud, later)

The OpenTelemetry Collector (or Grafana Alloy) on each host receives on
localhost and forwards over TLS with the backend's key:

```yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: localhost:4318
processors:
  batch: {}
exporters:
  otlphttp:
    endpoint: ${env:BACKEND_OTLP_ENDPOINT}
    headers:
      authorization: ${env:BACKEND_OTLP_AUTH}
service:
  pipelines:
    traces: { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
    metrics: { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
    logs: { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
```

`BACKEND_OTLP_ENDPOINT` and `BACKEND_OTLP_AUTH` are the collector's, from
the host's secrets; the header name is the backend's.

## Done when

On the host, with the receiver up and the `[telemetry]` table above in
`~/.config/sluice/config.toml`:

```sh
sluice step <branch>      # in a repository sluice serves
```

- The backend shows service `sluice` with a `sluice.round` trace whose
  `sluice.branch` is that branch, and the metric `sluice.queue.depth`.
- After a submission is tested with moon, it shows service `moon` with
  spans whose `sluice.trace_id` is a `sluice.round`'s trace id.
- After a submission fails its gate, it shows a log record of service
  `sluice` with `sluice.task` the failed task, whose trace id is a
  `sluice.round`'s: at `summary` its body names where the output is
  kept; at `tail` it is the output's end.
- The host's `host.name` and the table's `attributes` appear on all of
  them.
- No `SLUICE_OTEL*` and no API key appears in the environment of a
  session or of sluice.
