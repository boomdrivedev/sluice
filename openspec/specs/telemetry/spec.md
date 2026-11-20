# telemetry Specification

## Purpose

Lets an operator see how a branch's queue behaves and why a combination
failed, from a backend the host already runs, without anything sluice
sends becoming something it relies on (ADRs 0014, 0022).

## Requirements

### Requirement: Telemetry is off unless asked

sluice SHALL send no telemetry unless one of these holds:

- The command's own `--otel` or `SLUICE_OTEL` turns it on.
- The command gives no such choice and the host's `[telemetry]` holds
  `send = true`.

`SLUICE_OTEL` takes `1`, `true`, `yes` or `on` to turn telemetry on,
and `0`, `false`, `no` or `off` to turn it off. While telemetry is off,
a run's checks SHALL get none of sluice's `OTEL_*` variables.

#### Scenario: A command's choices are read from its flags and variables

- **WHEN** a command is parsed with no telemetry flag or variable, with
  each flag, and with each `SLUICE_OTEL*` variable
- **THEN** it chooses nothing, then what each flag gives, then what each
  variable gives, `SLUICE_OTEL` at `1`, `true`, `yes` or `on` choosing
  on and at `0` or `off` choosing off
  (`telemetry_is_off_unless_asked` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A run gets nothing while telemetry is off

- **WHEN** telemetry is off and a run's environment is built
- **THEN** it holds no `OTEL_*` variable, no `TRACEPARENT` and no
  `sluice.*` resource attribute (`a_run_gets_nothing_while_telemetry_is_off`
  in `crates/sluice/src/telemetry.rs`)

### Requirement: A round is a trace

While telemetry is on and sends traces, each round of a branch SHALL be
one trace rooted at `sluice.round`, with `sluice.recover`,
`sluice.decide` and `sluice.perform` beneath it, and `sluice.form`,
`sluice.start`, `sluice.verdict`, `sluice.blame` and `sluice.land`
beneath those as the round does each. Spans SHALL carry, where they
apply, `sluice.branch`, `sluice.submissions`, `sluice.candidate`,
`sluice.executor`, `sluice.confined`, `sluice.outcome`, `sluice.ran` and
`sluice.replayed`.

#### Scenario: A submission lands

- **WHEN** a submission is tested, passes and lands
- **THEN** its rounds report `sluice.round`, `sluice.recover`,
  `sluice.decide`, `sluice.perform`, `sluice.form`, `sluice.start`,
  `sluice.verdict` and `sluice.land`, and no `sluice.blame`
  (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A submission is blamed

- **WHEN** a submission's combination fails its gate
- **THEN** its rounds report `sluice.blame` and no `sluice.land`
  (`a_failure_reports_its_culprit` in `crates/sluice/tests/telemetry.rs`)

### Requirement: A branch's queue is measured

While telemetry is on and sends metrics, sluice SHALL send, each with
`sluice.repository` and `sluice.branch`, the gauges `sluice.queue.depth`
(submissions not yet out of the queue) and `sluice.window` (combinations
tested at once), and the counters `sluice.verdicts` by `sluice.outcome`,
`sluice.tasks.ran`, `sluice.tasks.replayed`, `sluice.culprits`
(submissions blamed), `sluice.conflicts`, `sluice.landed` and
`sluice.refused` (landings refused because the branch moved).

#### Scenario: A landing's metrics

- **WHEN** a submission is tested, passes and lands
- **THEN** sluice reports `sluice.queue.depth`, `sluice.verdicts`,
  `sluice.tasks.ran` and `sluice.landed`, each naming the repository and
  the branch (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A blamed submission is counted

- **WHEN** a submission's combination fails its gate
- **THEN** sluice reports `sluice.culprits` (`a_failure_reports_its_culprit`
  in `crates/sluice/tests/telemetry.rs`)

### Requirement: Waiting and testing are timed

While telemetry is on and sends metrics, sluice SHALL send, with
`sluice.repository` and `sluice.branch`, the histograms
`sluice.submission.landed_after` (seconds from submit to landed) and
`sluice.candidate.tested_for` (seconds a combination was under test, by
`sluice.outcome`), each declared in seconds, the unit `s`.

#### Scenario: A landing is timed

- **WHEN** a submission is tested, passes and lands
- **THEN** sluice reports `sluice.submission.landed_after` and
  `sluice.candidate.tested_for`
  (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A time is sent in seconds

- **WHEN** sluice's meter is sent `sluice.submission.landed_after`,
  `sluice.candidate.tested_for`, `sluice.tasks.time_saved` and
  `sluice.landed` as the core emits them
- **THEN** the first three are exported with the unit `s`, and
  `sluice.landed` with none (`a_time_is_sent_in_seconds` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A run's checks join the round

While telemetry is on, moon SHALL run with `--otel`, and a run's
environment SHALL carry:

- the collector sluice sends to and the traces and logs it sends;
- `OTEL_RESOURCE_ATTRIBUTES` extended with `sluice.place` and
  `sluice.revision`, each value percent-encoded where the variable's
  syntax would read it;
- while it sends traces, `sluice.trace_id` and `sluice.span_id` there,
  and `TRACEPARENT` naming the span that started the run.

Nothing of telemetry SHALL be added while telemetry is off.

#### Scenario: A resource attribute's value is encoded

- **WHEN** an attribute's value holds a comma, an equals sign or a space
- **THEN** each is percent-encoded and a `/` is left as it is
  (`values_are_encoded_where_the_syntax_would_read_them` in
  `crates/sluice/src/telemetry.rs`)

#### Scenario: A run joins the round

- **WHEN** telemetry is on with no table, `OTEL_RESOURCE_ATTRIBUTES`
  holds `deployment.environment=test`, and a run is started under a
  span at a place named `p 1`
- **THEN** its environment holds the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_METRICS_EXPORTER=none`, a
  `TRACEPARENT` naming the span, and `OTEL_RESOURCE_ATTRIBUTES` with
  `deployment.environment=test`, `host.name`, the trace and span ids,
  `sluice.place=p%201` and the revision
  (`a_run_s_checks_join_the_round` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A run joins no trace while traces are off

- **WHEN** telemetry is on, `OTEL_TRACES_EXPORTER` is `none`, and a run
  is started under a span
- **THEN** its environment holds `OTEL_TRACES_EXPORTER=none`,
  `OTEL_METRICS_EXPORTER=none`, no `TRACEPARENT`, and
  `OTEL_RESOURCE_ATTRIBUTES` with the host's own attributes,
  `host.name`, `sluice.place` and `sluice.revision`, and no
  `sluice.trace_id` or `sluice.span_id`
  (`with_traces_off_a_run_s_checks_join_no_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: A failure's output is a log record under its verdict

While telemetry sends logs and `SLUICE_OTEL_LOGS` is not `off`, the round
that reads a failed task's verdict SHALL send one OTLP log record for
it, under its `sluice.verdict` span while traces are sent, with
`sluice.branch`, `sluice.candidate`, `sluice.submissions`,
`sluice.task`, `sluice.kept` (where the output is kept), `sluice.bytes`
and `sluice.replayed` (20). A preview whose command failed SHALL send
one, with `sluice.preview`, when it is released. A passing combination
SHALL send none.

#### Scenario: A failed task's record sits under its verdict

- **WHEN** a submission's combination fails its gate on task `tests:t-a`
- **THEN** exactly one record is sent, its `sluice.task` is `tests:t-a`,
  and its trace and span ids are those of the `sluice.verdict` span
  (`a_failure_s_output_is_sent_under_its_verdict_scrubbed` in
  `crates/sluice/tests/telemetry_logs.rs`; `a_failure_reports_its_culprit`
  in `crates/sluice/tests/telemetry.rs`)

#### Scenario: A failed task's record names no span while traces are off

- **WHEN** `OTEL_TRACES_EXPORTER` is `none` and a submission's
  combination fails its gate on task `tests:t-a`
- **THEN** no span is sent, and exactly one failure's record is sent,
  its `sluice.task` `tests:t-a` and with no trace or span id
  (`with_traces_off_a_failure_s_record_names_no_span` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A landing sends no failure's record

- **WHEN** a submission is tested, passes and lands
- **THEN** no output event is emitted
  (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: Only a failure's output and sluice's warnings are logs

The log records sluice sends SHALL be a failure's output and sluice's
own warnings and errors, each also printed on stderr as before. A
metric's event SHALL never become a log record. With `SLUICE_OTEL_LOGS`
`off`, no log record SHALL be sent at all.

#### Scenario: No metric becomes a log

- **WHEN** a submission's combination fails its gate with logs on
- **THEN** every record sent is a failure's output or a warning or
  error, though the round emitted metric events
  (`a_failure_s_output_is_sent_under_its_verdict_scrubbed` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: How much of a failure is sent is the host's choice

The logs' level SHALL be `off`, `summary` or `tail`, `summary` by
default. The command's `--otel-logs` or `SLUICE_OTEL_LOGS` chooses it,
else the host's `[telemetry] logs`. At `summary` a failure's record
body SHALL name the task and where its output is kept, never the
output. At `tail` it SHALL be the output's end, beginning at a
character's start, at most the tail's bytes chosen and never more than
65536. A warning's body SHALL never be summarised.

#### Scenario: A summary names where the output is kept

- **WHEN** task `a:test` fails at `summary` and its output is kept at
  `/x/.logs/a.log`
- **THEN** the body is `a:test failed; its output is kept at
  /x/.logs/a.log` and a warning's body is sent scrubbed, not summarised
  (`a_summary_sends_where_the_output_is_kept_never_the_output` in
  `crates/sluice/src/telemetry.rs`)

#### Scenario: A tail ends on whole characters

- **WHEN** the limit is 2 bytes and the output is `abcdéf`
- **THEN** the body is `f`, and a limit over 65536 is held to 65536
  (`a_tail_keeps_the_end_from_a_character_s_start` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: Every log body is scrubbed before it leaves

Before a record is sent, sluice SHALL replace with `[redacted]` the
value, eight bytes or longer, of any variable in its environment whose
name holds `TOKEN`, `SECRET`, `KEY`, `PASS`, `AUTH`, `CREDENTIAL` or
`HEADERS`, and anything shaped like a GitHub, AWS, Slack or Anthropic
token, a JWT, a bearer token or a private key block. The output kept on
disk SHALL be unchanged.

#### Scenario: A secret variable's value is never sent

- **WHEN** `SLUICE_TEST_TOKEN` holds the start of what a failed check
  prints and logs are at `tail`
- **THEN** the record's body begins `[redacted] in `
  (`a_failure_s_output_is_sent_under_its_verdict_scrubbed` in
  `crates/sluice/tests/telemetry_logs.rs`;
  `a_secret_variable_s_value_is_redacted_wherever_it_appears` in
  `crates/sluice/src/telemetry.rs`)

#### Scenario: A token's shape is redacted from any variable

- **WHEN** a body holds a GitHub token, an AWS key id, a Slack token, an
  Anthropic key, a JWT, a bearer token or a private key block
- **THEN** each becomes `[redacted]` and `ok: 3 passed` is left as it is
  (`what_reads_as_a_secret_is_redacted` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: Telemetry decides nothing

Nothing sluice decides SHALL read telemetry back; the records are the
only truth (core.11, core.14). An exporter that cannot reach its
collector SHALL lose what it held without failing the command, and a
record lost on the way SHALL never be sent again. A submission's outcome
SHALL be the same with telemetry on or off.

#### Scenario: A round decides as it would with telemetry off

- **WHEN** a submission is tested with telemetry on and its gate passes,
  or fails
- **THEN** `sluice submit` exits landed, or failed, as the CLI's tests
  show it does with telemetry off
  (`a_landing_reports_its_round_verdict_and_landing` and
  `a_failure_reports_its_culprit` in `crates/sluice/tests/telemetry.rs`)

### Requirement: OpenTelemetry's settings turn a signal off

While telemetry is on, sluice SHALL send a signal only while both the
host's table and OpenTelemetry's SDK environment variables leave it on.
`OTEL_SDK_DISABLED` `true` SHALL turn every signal off, and
`OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` or `OTEL_LOGS_EXPORTER`
`none` SHALL turn that one alone off. Each value SHALL be read without
its case or the space around it, and any other value SHALL leave its
signal as the table has it.

#### Scenario: Only traces are turned off

- **WHEN** telemetry is on, `OTEL_TRACES_EXPORTER` is `none`, and a
  submission's combination fails its gate
- **THEN** nothing is sent to `/v1/traces`, and its metrics are sent to
  `/v1/metrics` and its failure's record to `/v1/logs`
  (`with_traces_off_metrics_and_logs_still_go_to_the_endpoint` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: Only metrics are turned off

- **WHEN** telemetry is on, `OTEL_METRICS_EXPORTER` is `none`, and a
  submission's combination fails its gate
- **THEN** nothing is sent to `/v1/metrics`, and its spans are sent to
  `/v1/traces` and its failure's record to `/v1/logs`
  (`with_metrics_off_traces_and_logs_still_go_to_the_endpoint` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: Only logs are turned off

- **WHEN** telemetry is on with logs at `summary`, `OTEL_LOGS_EXPORTER`
  is `none`, and a submission's combination fails its gate
- **THEN** nothing is sent to `/v1/logs`, and its spans are sent to
  `/v1/traces` and its metrics to `/v1/metrics`
  (`with_logs_off_traces_and_metrics_still_go_to_the_endpoint` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A setting is read as OpenTelemetry reads it

- **WHEN** the settings are read from an environment
- **THEN** each setting reads as follows
  (`the_standard_settings_are_read_as_opentelemetry_reads_them` in
  `crates/sluice/src/telemetry.rs`):
  - `OTEL_TRACES_EXPORTER` at `none`, `NONE` or ` None ` turns traces
    alone off;
  - at `otlp`, `console` or unset it leaves them on;
  - `OTEL_SDK_DISABLED` at `true` or ` TRUE ` turns every signal off;
  - at `1`, `false` or empty it turns none off.

### Requirement: Telemetry with every signal off is off

Telemetry SHALL be off as if it had not been asked for, whatever the
command's choice or the table's `send` say, while either of these
holds:

- `OTEL_SDK_DISABLED` is `true`;
- every signal is turned off, by the table or the environment.

Then sluice SHALL send nothing, a run's checks SHALL get none of
sluice's `OTEL_*` and no `TRACEPARENT`, moon SHALL run without
`--otel`, and nothing SHALL be said of it.

#### Scenario: The SDK turned off sends nothing

- **WHEN** telemetry is asked for, `OTEL_SDK_DISABLED` is `true`, and a
  submission's combination fails its gate
- **THEN** nothing reaches the collector, telemetry reads as off, a
  run's environment gets nothing of telemetry, and `sluice submit` exits
  failed as it does with telemetry off
  (`with_the_sdk_disabled_nothing_is_sent` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: The host's settings say where telemetry goes

The host's own settings, `~/.config/sluice/config.toml`, SHALL take a
`[telemetry]` table, read with the file's other tables (151, C.7;
record 0106; rule core.53). Its keys SHALL be `send`, `collector`,
`service`, `traces`, `metrics`, `logs`, `logs_tail_bytes` and
`attributes`. A key left unset SHALL take sluice's default: nothing
sent, no collector, service `sluice`, traces and metrics on, logs at
`summary` and a tail of 8192 bytes. Branch protection's hook SHALL pass
over the table.

#### Scenario: The table read as written

- **WHEN** the host's settings hold `[telemetry]` with `send = true`,
  `collector = "http://localhost:4318/"`, `service = "sluice-a"`,
  `traces = false`, `metrics = true`, `logs = "tail"`,
  `logs_tail_bytes = 4096` and `attributes` with
  `"deployment.environment" = "prod"`
- **THEN** the settings give telemetry sent, its collector
  `http://localhost:4318`, service `sluice-a`, traces off, metrics on,
  logs at `tail` and at most 4096 bytes, and the one attribute; and
  settings with no `[telemetry]` give no table
  (`the_telemetry_table_is_read_as_written` in
  `crates/sluice/src/instance.rs`)

### Requirement: A telemetry setting that does not read is refused

sluice SHALL refuse the host's settings, naming `[telemetry]` and the
key, where the table holds a field it does not know, a `collector` that
is no `http://` address naming a host, an empty `service`, a `logs`
other than `off`, `summary` or `tail`, a `logs_tail_bytes` outside 1 to
65536, or an attribute with an empty name or named `service.name` or
`service.version`.

#### Scenario: A telemetry setting that does not read

- **WHEN** the table holds `collector = "https://otel.example:4318"`,
  `collector = "localhost:4318"`, `logs = "all"`,
  `logs_tail_bytes = 0`, `logs_tail_bytes = 65537`, `service = ""`,
  `attributes` naming `service.name`, or a field `headers`, each alone
- **THEN** each is refused, naming `[telemetry]` and its key; the
  `https://` collector's refusal says sluice sends plain HTTP to a
  receiver on the host; the bytes' refusal names 1 and 65536; and the
  attribute's names `[telemetry] service`
  (`a_telemetry_setting_that_does_not_read_is_refused` in
  `crates/sluice/src/instance.rs`)

### Requirement: A command chooses for itself alone

A command's own `--otel` flags and `SLUICE_OTEL` variables SHALL
choose whether telemetry is sent, the service, the logs' level and the
tail's bytes for that command alone, over the host's table. Where the
command gives none of a setting, the table's SHALL hold, else sluice's
default. `--otel` alone SHALL turn telemetry on, and `--otel=false` or
`SLUICE_OTEL` at `0`, `false`, `no` or `off` SHALL turn it off for that
command, whatever the table says.

#### Scenario: Each setting from the command, else the table, else its default

- **WHEN** the settings are chosen from what a command was given and
  the host's table
- **THEN** each comes out as follows
  (`the_settings_are_chosen_by_the_command_then_the_table` in
  `crates/sluice/src/telemetry.rs`):
  - with no table and no flag, nothing is sent;
  - with `send = true` and no flag, telemetry is sent;
  - with `send = true` and `SLUICE_OTEL` at `0`, nothing is sent;
  - with no table, or `send = false`, and `--otel`, telemetry is sent;
  - the service, the logs' level and the tail's bytes each come from
    the command's choice where it gives one, else from the table's
    `service`, `logs` and `logs_tail_bytes`, else `sluice`, `summary`
    and 8192.

#### Scenario: --otel takes no value or a boolean

- **WHEN** the command line is `sluice status --otel main`,
  `sluice status --otel=false main` or `sluice status main`, or
  `sluice --otel` alone
- **THEN** the telemetry choice is on, off or not given, in that order,
  and the command is `status` of `main` each time; and `sluice --otel`
  is the home view with telemetry on
  (`otel_takes_no_value_or_a_boolean` in `crates/sluice/src/lib.rs`)

### Requirement: The table's collector is sluice's alone

Where the host's table names a `collector`, sluice SHALL send there and
nowhere else, with none of the caller's `OTEL_EXPORTER_OTLP_*`
endpoints, headers, protocol, timeout or compression. With no table, or
for a command's own `--otel` beside a table that names none, the
environment's `OTEL_EXPORTER_OTLP_*` SHALL name the collector, its
headers and its protocol. A log body SHALL still be scrubbed of the
values of the caller's secret-named variables.

#### Scenario: A session's collector and headers never take sluice's telemetry

- **WHEN** the table sends to one collector, and the caller's
  environment names another as `OTEL_EXPORTER_OTLP_ENDPOINT`, holds
  `OTEL_EXPORTER_OTLP_HEADERS` with a header `x-session`, and names
  `OTEL_EXPORTER_OTLP_PROTOCOL` `grpc`; and a submission's combination
  fails its gate
- **THEN** the table's collector is sent protobuf at `/v1/traces`,
  `/v1/metrics` and `/v1/logs`, and no request carries `x-session`; the
  other collector is sent nothing; and `sluice submit` exits failed
  (`the_tables_collector_takes_sluices_telemetry_and_none_of_the_callers`
  in `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A table with no collector sends where the environment says

- **WHEN** the table names a `service` and neither `send` nor
  `collector`, the command is given `--otel`, the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT` names a collector, and a submission's
  combination fails its gate
- **THEN** telemetry is sent, and that collector is sent its traces,
  metrics and logs
  (`with_no_collector_in_the_table_the_environment_names_it` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: sluice names itself and its host

Everything sluice sends SHALL carry `service.name`, the service chosen,
and `service.version`, the release's. It SHALL also carry `host.name`,
the host's own name, unless the table's `attributes` names one, or,
with no table, `OTEL_RESOURCE_ATTRIBUTES` does. Where the host has a
table, its resource SHALL be those and the table's `attributes`, and
`OTEL_RESOURCE_ATTRIBUTES` and `OTEL_SERVICE_NAME` SHALL not be read.
With no table, `OTEL_RESOURCE_ATTRIBUTES` SHALL add the host's own, as
before.

#### Scenario: The resource with a table and without

- **WHEN** the resource is made four ways:
  - with a table whose `attributes` hold
    `deployment.environment = "prod"`, beside `OTEL_RESOURCE_ATTRIBUTES`
    `deployment.environment=session`;
  - with a table whose `attributes` name `host.name`;
  - with no table and `OTEL_RESOURCE_ATTRIBUTES`
    `deployment.environment=session`;
  - with no table and `OTEL_RESOURCE_ATTRIBUTES` naming `host.name`
- **THEN** each comes out as follows
  (`the_resource_is_the_tables_and_names_the_host` in
  `crates/sluice/src/telemetry.rs`):
  - the first is the table's service, the release, `host.name` the
    host's name and `deployment.environment` `prod`, and reads no
    variable;
  - the second names the host by the table's `host.name` alone;
  - the third reads the variable, and sluice adds `host.name` the
    host's name;
  - the fourth reads the variable, and sluice adds no `host.name`.

### Requirement: Telemetry goes as OTLP to the collector chosen

While telemetry is on, sluice SHALL send it as OTLP over HTTP,
protobuf, to the collector: the table's `collector`, else
`OTEL_EXPORTER_OTLP_ENDPOINT`, else `http://localhost:4318`.

#### Scenario: Telemetry reaches the collector as OTLP over HTTP

- **WHEN** telemetry is on with no table, `OTEL_EXPORTER_OTLP_ENDPOINT`
  names a collector, the service chosen is `sluice-under-test`, and a
  submission's combination fails its gate
- **THEN** the collector is sent a `POST` of `application/x-protobuf`
  at `/v1/traces`, `/v1/metrics` and `/v1/logs`, and the traces name
  the service, the release and `sluice.verdict`
  (`telemetry_goes_to_the_endpoint_as_otlp_over_http` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: The table turns a signal off

The host's table SHALL turn traces off with `traces = false` and
metrics with `metrics = false`, and logs SHALL be off while the logs'
level chosen is `off`. The environment SHALL never turn on a signal the
table turns off.

#### Scenario: The table turns traces off whatever the environment says

- **WHEN** the table holds `send = true`, its collector and
  `traces = false`, `OTEL_TRACES_EXPORTER` is `otlp`, and a
  submission's combination fails its gate
- **THEN** nothing is sent to `/v1/traces`, and its metrics are sent to
  `/v1/metrics` and its failure's record to `/v1/logs`
  (`the_table_turns_traces_off_whatever_the_environment_says` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: The table and the environment together

- **WHEN** the signals are chosen from a table and an environment
- **THEN** each comes out as follows
  (`the_table_turns_a_signal_off_and_the_environment_never_turns_it_on`
  in `crates/sluice/src/telemetry.rs`):
  - `traces = false` with `OTEL_TRACES_EXPORTER` at `otlp` leaves
    traces off;
  - `traces = true` with `OTEL_TRACES_EXPORTER` at `none` leaves them
    off;
  - `metrics` unset with nothing in the environment leaves metrics on;
  - `logs = "off"` leaves logs off whatever the environment says.

### Requirement: With a table, a run gets what sluice sends

With a `[telemetry]` table, a run's `OTEL_*` SHALL be built from what
sluice sends and from none of the caller's other `OTEL_*`:

- the table's `collector` as `OTEL_EXPORTER_OTLP_ENDPOINT`, with
  protocol `http/protobuf`, else the caller's `OTEL_EXPORTER_OTLP_*`;
- `OTEL_<SIGNAL>_EXPORTER` at `none` for each signal sluice does not
  send, traces, metrics or logs;
- `OTEL_RESOURCE_ATTRIBUTES` holding `host.name` and the table's
  `attributes`, before the run's own, and never `service.name`.

#### Scenario: A run gets the table's collector and none of the caller's

- **WHEN** telemetry is on with the table's `collector`,
  `metrics = false` and `attributes` holding
  `deployment.environment = "prod"`, the caller's environment holds
  another `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`
  and `OTEL_RESOURCE_ATTRIBUTES`, and a run is started under a span
- **THEN** its environment holds `OTEL_EXPORTER_OTLP_ENDPOINT` the
  table's collector, `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`,
  `OTEL_METRICS_EXPORTER=none`, a `TRACEPARENT` naming the span, and
  `OTEL_RESOURCE_ATTRIBUTES` exactly `host.name`,
  `deployment.environment=prod`, the trace and span ids,
  `sluice.place` and `sluice.revision`; and no
  `OTEL_EXPORTER_OTLP_HEADERS` and nothing of the caller's attributes
  (`with_the_tables_collector_a_run_gets_it_and_none_of_the_callers` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: With no table, a run gets the caller's telemetry

Where the host has no `[telemetry]` table, a run SHALL get the caller's
`OTEL_*` as sluice read them when it started, but for
`OTEL_METRICS_EXPORTER`, which is `none`; its `OTEL_RESOURCE_ATTRIBUTES`
holding `host.name` where sluice named the host itself.

#### Scenario: A run gets the caller's collector and attributes

- **WHEN** telemetry is on with no table, the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT` names a collector and its
  `OTEL_RESOURCE_ATTRIBUTES` holds `deployment.environment=test`, and a
  run is started under a span
- **THEN** its environment holds the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT`, and `OTEL_RESOURCE_ATTRIBUTES` begins
  `deployment.environment=test,host.name=host-a`
  (`a_run_s_checks_join_the_round` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: A table that sends names its collector

sluice SHALL refuse the host's settings where `[telemetry]` holds
`send = true` and no `collector`, naming `[telemetry] collector` and
`send = true`, and saying to name the `http://` address sluice sends to
(151; record 0106 as amended; rule core.53). A table with `send` unset
or `false` SHALL need no `collector`.

#### Scenario: A table that sends with no collector

- **WHEN** the table holds `send = true` alone, or `send = true` with
  `service` and `logs` and no `collector`; or `send = false` alone, or
  `service` alone
- **THEN** each of the first two is refused, naming
  `[telemetry] collector`, `send = true` and `http://`; and each of the
  last two is read, its `send` as written and no collector
  (`a_telemetry_table_that_sends_names_its_collector` in
  `crates/sluice/src/instance.rs`)

### Requirement: A run's tools send no metrics

While telemetry is on, a run's environment SHALL hold
`OTEL_METRICS_EXPORTER=none`, with a table or without, whether or not
sluice sends metrics and whatever the caller's environment holds, so the
run's ids on its resource ride on no metric (151 and record 0014, each
as amended 2026-11-18; rule core.66).

#### Scenario: A run's tools send no metrics

- **WHEN** a run's environment is built under a span with telemetry on,
  four ways: a table sending metrics; a table with `metrics = false`; no
  table; and no table with the caller's `OTEL_METRICS_EXPORTER` at
  `otlp`
- **THEN** each holds `OTEL_METRICS_EXPORTER=none`, and its
  `OTEL_RESOURCE_ATTRIBUTES` the trace and span ids, `sluice.place` and
  `sluice.revision` (`a_run_s_tools_send_no_metrics` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A metric carries only its repository, branch and outcome

Each metric sluice sends SHALL carry `sluice.repository` and
`sluice.branch`, `sluice.outcome` where it has one, and no other
attribute, so none names a run's trace, span, revision or place, a
submission, a submitter or a task (record 0014 as amended 2026-11-18;
rule core.66).

#### Scenario: A round's metrics carry nothing of a run

- **WHEN** a submission is tested, passes and lands; one is blamed; one
  is replayed; one conflicts; and a landing is refused
- **THEN** every metric each reports names `sluice.repository` and
  `sluice.branch`, and carries no field but those and `sluice.outcome`
  (`a_landing_reports_its_round_verdict_and_landing`,
  `a_failure_reports_its_culprit`, `a_replay_is_counted`,
  `a_conflict_is_counted` and `a_refused_landing_is_counted` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: A metric names its repository by its origin, else its folder

`sluice.repository` SHALL be the last two names of the path of the
repository's `origin`, where its URL names a host, a trailing `.git`
dropped; else the repository's folder's name, `.git` dropped. No other
part of the origin's URL SHALL be kept.

#### Scenario: A repository's name

- **WHEN** a repository is named from its origin and its common
  directory
- **THEN** each comes out as follows
  (`a_repository_is_named_by_its_origin_else_its_folder` in
  `crates/sluice/src/profiles/local_refs.rs`):
  - `https://user:token@github.com/acme/app.git`,
    `git@github.com:acme/app.git` and
    `ssh://git@git.example:2222/acme/app/` each give `acme/app`;
  - no origin with the common directory `/src/app/.git` gives `app`;
  - the origin `/srv/git/app.git` with the common directory
    `/srv/q/app.git` gives `app`;
  - no name holds `user`, `token` or a host.

### Requirement: What a replay saved is measured

While telemetry sends metrics, the round that reads a run's verdict
SHALL add to the counter `sluice.tasks.time_saved`, in seconds with the
unit `s`, the time each replayed task's key took when it last ran: the
`took_ms` of the newest pass the verdict counts for its key. A replay
that no counted pass proves, or whose pass recorded no time, SHALL add
nothing.

#### Scenario: A replay saves its key's last time

- **WHEN** a verdict is read of a run that replayed three keys: one whose
  counted passes took 4000 ms and later 6000 ms, one whose counted pass
  recorded no time, and one with no counted pass
- **THEN** it reads 6000 ms saved, and the third key unproven
  (`a_replay_saves_the_time_its_key_last_took` in
  `crates/sluice-core/src/verdict.rs`)

#### Scenario: A round counts what its replays saved

- **WHEN** `lib:test` takes 4000 ms when it runs, and a submission
  brings `lib/` back to a tree a landing before had, so the task is
  replayed
- **THEN** sluice reports `sluice.tasks.time_saved` of 4 seconds with
  the repository and the branch (`a_replay_counts_the_time_it_saved` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: A command started under a caller's trace nests in it

A command SHALL read its caller's W3C trace context from `TRACEPARENT`
and, where given, `TRACESTATE` in its environment as it starts; one
that does not read as W3C's trace context SHALL be none (153). While
telemetry sends traces, a command started under one SHALL send one span
named `sluice <command>`, such as `sluice exec` or `sluice job run`,
whose parent is the caller's span, holding `sluice.exit`, its exit. A
command started under none SHALL send no span of its own.

#### Scenario: A command nests under its caller

- **WHEN** telemetry sends traces, `sluice submit` is started under
  `TRACEPARENT` `00-<t>-<s>-01`, and another submit under none
- **THEN** one `sluice submit` span is sent in trace `<t>` with parent
  `<s>` and the first submit's exit as `sluice.exit`, and no
  `sluice submit` span is sent for the second
  (`a_command_nests_under_its_caller_s_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A context is read as W3C reads it

- **WHEN** `TRACEPARENT` is `00-` then 32 hex digits, 16 hex digits and
  `01`, beside a `TRACESTATE`; or holds an all-zero trace id, hex in
  upper case, version `ff`, or three fields
- **THEN** the first reads as a context with its trace state and its
  `TRACEPARENT` as given, and each of the others reads as none
  (`a_trace_context_is_read_as_w3c_reads_it` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A caller's trace turns nothing on

A caller's trace context SHALL name only where sluice's spans nest. It
SHALL never turn telemetry or a signal on: whether sluice sends is the
command's own choice, else the host's `[telemetry]`, as before (151;
rule core.53).

#### Scenario: A context with nothing asked sends nothing

- **WHEN** the command gives no telemetry choice, the host has no
  `[telemetry]`, and the environment holds a `TRACEPARENT`
- **THEN** the settings chosen send nothing
  (`a_caller_s_context_turns_nothing_on` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A round is a trace of its own under any command

Each round SHALL be one trace of its own, its `sluice.round` with no
parent, whatever process runs it and under whatever command's span.
sluice's host process SHALL keep no `TRACEPARENT` or `TRACESTATE` of
the command that starts it.

#### Scenario: A round run under a command's span

- **WHEN** telemetry sends traces and a submit started under a caller's
  context waits while its combination is tested and lands
- **THEN** each `sluice.round` span has no parent and a trace that is
  neither the caller's nor another round's
  (`a_command_nests_under_its_caller_s_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: The host process keeps no caller's context

- **WHEN** the host process's environment is chosen from a command's
  that holds `TRACEPARENT` and `TRACESTATE`
- **THEN** it keeps neither
  (`the_host_process_keeps_no_caller_s_trace_context` in
  `crates/sluice/src/host_process/environment.rs`)

### Requirement: A submission keeps the trace it was submitted under

A submission SHALL keep on its record, as `trace`, the `TRACEPARENT` of
the context its submit was started under, whether or not the submit
sends telemetry; each submission of one submit of several worktrees
SHALL keep the same. A submission submitted under none, and a pull
request's taken in from the forge, SHALL keep none. Nothing sluice
decides SHALL read it (153).

#### Scenario: Each submission keeps its submit's context

- **WHEN** with telemetry off, `sluice submit` is started under
  `00-<t>-<s>-01`, a submit of two worktrees as one act under it, and a
  submit under none
- **THEN** the first submission's record holds `trace`
  `00-<t>-<s>-01`, each of the act's two holds it, and the last's holds
  no `trace`
  (`a_submission_keeps_the_trace_it_was_submitted_under` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A round that tests a submission links to its trace

While telemetry sends traces, each `sluice.start` span and each
`sluice.verdict` span of a combination SHALL hold one link to the
trace each submission it holds kept, the link holding
`sluice.submission`, that submission's number (153; rule core.53).

#### Scenario: A combination's start and verdict link to its work's caller

- **WHEN** a combination holds submission 1, submitted under
  `00-<t>-<s>-01`, and submission 2, submitted under none, and is
  tested and lands
- **THEN** each of its `sluice.start` and `sluice.verdict` spans holds a
  link to span `<s>` of trace `<t>` with `sluice.submission` 1, and no
  link for submission 2
  (`a_round_links_to_the_trace_its_work_was_submitted_under` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: A finished run's checks keep their runner's times

The check seam SHALL answer, for a run that has ended and while its
directory stands, each of its checks as its runner timed it: moon's
from moon's records in the place, as while the run went, and the
`command` runner's one check from the run's own start and end. It is
never a result (37, 38).

#### Scenario: A finished command run is timed by its start and end

- **WHEN** a run of the `command` runner has ended
- **THEN** the check seam's times for it hold its one check, started
  when the run started and ended when it ended
  (`a_finished_command_run_is_timed_by_its_start_and_end` in
  `crates/sluice/tests/checks.rs`)

#### Scenario: A finished moon run's checks keep their times

- **WHEN** a moon run of `alpha:test` and `beta:test` has ended
- **THEN** the check seam's times for it hold each with its start and
  its end, its start no later than its end
  (`a_finished_moon_run_s_checks_are_timed` in
  `crates/sluice/tests/profiles/moon.rs`)

### Requirement: An exec's span holds each check it ran

While telemetry sends traces, `sluice exec` started under a caller's
context SHALL send beneath its `sluice exec` span one `sluice.check`
span for each check its run ran whose runner gave its start and end,
over those times, with `sluice.task` and `sluice.outcome`, `pass` or
`fail` as the run's report has it. A check answered from an earlier
run, or with no time from its runner, SHALL have none, and the
`sluice exec` span SHALL hold `sluice.ran` and `sluice.replayed`.

#### Scenario: Each check run is a span over its runner's times

- **WHEN** `sluice exec` started under a caller's context runs
  `tests:t-a`, which passes from 12:00:01 to 12:00:04, `tests:t-b`,
  which fails, and `tests:t-c`, answered from an earlier run
- **THEN** its `sluice exec` span holds a `sluice.check` span for
  `tests:t-a` from 12:00:01 to 12:00:04 with `sluice.outcome` `pass`,
  one for `tests:t-b` with `fail`, and none for `tests:t-c`, and holds
  `sluice.ran` 2 and `sluice.replayed` 1
  (`an_exec_s_span_holds_each_check_it_ran` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: An exec's command is one check, named scrubbed

A command `sluice exec` runs after `--` SHALL be its run's one check,
its `sluice.check` span's `sluice.task` the command's words with
whatever reads as a secret replaced by `[redacted]`, as a log's body
is.

#### Scenario: A command's secret never names its span

- **WHEN** `sluice exec` started under a caller's context runs
  `-- env API_TOKEN=ghp_0123456789abcdefghijABCDEFGHIJ012345 true`
- **THEN** its one `sluice.check` span's `sluice.task` holds
  `[redacted]` and not the token
  (`an_exec_s_command_is_one_check_named_scrubbed` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: An exec's checks nest under its span

While telemetry sends traces, the run `sluice exec` starts under a
caller's context SHALL be handed what a combination's run is handed
(151), its `TRACEPARENT` naming the `sluice exec` span in the caller's
trace.

#### Scenario: An exec's checks get its span through the binary

- **WHEN** the `sluice` binary runs `sluice exec --otel <branch> -- sh
  -c 'env > seen'` with `TRACEPARENT` `00-<t>-<s>-01` and a collector
- **THEN** `seen`'s `TRACEPARENT` names trace `<t>` and a span other
  than `<s>`, and the traces sent hold a `sluice exec` span whose
  parent is `<s>`
  (`an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` in
  `crates/sluice/tests/exec.rs`)

### Requirement: A submission keeps its time under test

Every write of a branch's records SHALL keep on each submission's
record `under_test_ms`, the milliseconds a combination on the branch's
revision holding it was under test, combinations that overlap counted
once, and `under_test_since`, since when one is; a submission's time
SHALL stop at its end. They SHALL be kept whether or not telemetry is
on, and nothing sluice decides SHALL read them (154).

#### Scenario: Combinations that overlap count once

- **WHEN** a submission's own combination and a deeper one holding it
  go under test at 12:00, the first is judged at 12:04 and the deeper
  one at 12:10
- **THEN** its record holds 600000 ms under test and no
  `under_test_since`
  (`a_submission_s_time_under_test_counts_overlapping_combinations_once`
  in `crates/sluice-core/src/line.rs`)

#### Scenario: A submission's time stops at its end

- **WHEN** a submission under test since 12:00 is withdrawn at 12:03
- **THEN** its record holds 180000 ms under test and no
  `under_test_since` (`a_submission_s_time_under_test_stops_at_its_end`
  in `crates/sluice-core/src/line.rs`)

#### Scenario: Every write keeps it

- **WHEN** a submission's combination goes under test, the clock
  advances four minutes, and the round that reads its verdict writes
- **THEN** its record holds 240000 ms under test
  (`every_write_keeps_a_submission_s_time_under_test` in
  `crates/sluice-core/src/step.rs`)

### Requirement: A run with no outcome is never its submission's failure

A run that its time limit did not end and in which no check passed or
failed SHALL count one on `runs_with_no_outcome` of each submission its
combination holds, and the submission's life SHALL name the count. It
SHALL never make that life's outcome `failed` or its status an error
(114; rule core.53).

#### Scenario: A run that decided nothing is counted

- **WHEN** runs end that no host took, whose every check is `none`,
  whose checks are `none` and `unproven`, that were given no check, that
  passed a check, that failed one, and that their time limit ended
- **THEN** the first four each count one on every submission their
  combination holds, and the last three none
  (`a_run_that_decided_nothing_is_counted_on_its_submissions` in
  `crates/sluice-core/src/line.rs`)

#### Scenario: A run no host took counts on its submissions

- **WHEN** a combination's run is ended at its time limit with no live
  host that may take it
- **THEN** each of the combination's submissions holds one run with no
  outcome, and none is blamed
  (`a_run_no_host_took_counts_as_no_outcome_on_its_submissions` in
  `crates/sluice-core/src/step.rs`)

### Requirement: A submission's life is a span

While telemetry sends traces, the write that ends a submission SHALL
send one span, `sluice.submission`, from its submission to that write,
whose parent is the trace it kept, else none, naming its repository,
branch, number, head, work, agent, outcome, seconds waiting and under
test, and runs with no outcome, and linking to the span that ended it.
Only `failed` SHALL be an error (154; rule core.53).

#### Scenario: A submission's life in the trace it was submitted under

- **WHEN** telemetry sends traces, a submission is made under
  `TRACEPARENT` `00-<t>-<s>-01` and lands, and another is made under
  none and lands
- **THEN** a `sluice.submission` span is sent for each, the first in
  trace `<t>` with parent `<s>`, the second the root of a trace of its
  own, each starting at its submission, with `sluice.outcome` `landed`
  and `sluice.waited_seconds` and `sluice.tested_seconds` that sum to
  its length
  (`a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under`
  in `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A life names how it ended

- **WHEN** a life is sent for a submission blamed, and one for a
  submission withdrawn after two runs with no outcome
- **THEN** the first span's status is an error, and the second's is
  not, with `sluice.outcome` `withdrawn` and `sluice.no_outcome` 2
  (`a_life_names_how_it_ended_and_its_runs_with_no_outcome` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A submission's end is measured

While telemetry sends metrics, the write that ends a submission SHALL
add one to `sluice.submissions.ended` and send its seconds waiting with
no combination of it under test, `sluice.submission.waited`, and under
test, `sluice.submission.tested`, each with `sluice.repository`,
`sluice.branch` and `sluice.outcome`, its end's word, and nothing that
names its head, its submitter or its trace (154; rules core.53,
core.66).

#### Scenario: Each end is found once

- **WHEN** a write moves submissions from `queued` to `landed`, from
  `testing` to `withdrawn`, from `failed` to `withdrawn` and from
  `refused` to `queued`, and writes one made `landed`
- **THEN** it names the first, the second and the last as ended, and
  no other (`a_write_names_the_submissions_it_ends` in
  `crates/sluice-core/src/line.rs`)

#### Scenario: Ends are measured by how they ended

- **WHEN** one submission lands, one is blamed and one is withdrawn
  from the queue
- **THEN** sluice reports `sluice.submissions.ended`,
  `sluice.submission.waited` and `sluice.submission.tested` for each,
  with `sluice.outcome` `landed`, `failed` and `withdrawn`, the
  withdrawn one tested 0 seconds, each naming the repository and the
  branch and no field outside `METRIC_KEYS`
  (`a_submission_s_end_is_measured` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: A run's room is held for its branch

A run's part of the host's bound SHALL be held for the branch it is
for: a combination's for its branch, a session's or a preview's for the
branch its run is held to. The host's registry of running checks SHALL
name it beside the repository; an ask or an entry that names none SHALL
count toward the bound under no branch.

#### Scenario: Runs are counted by branch

- **WHEN** a room holds two parts for `main` and one for `bolt/x` of
  one repository, and its registry holds an entry naming no branch
- **THEN** it counts two runs for `main`, one for `bolt/x`, and all
  four against its bound
  (`the_host_s_running_checks_are_counted_by_branch` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: A hold names its branch

- **WHEN** a hold naming `main` is written and read, and one an older
  sluice wrote with no branch is read
- **THEN** the first reads `main` and the second none
  (`a_hold_names_its_branch_and_an_older_one_none` in
  `crates/sluice/src/host_process/protocol.rs`)

### Requirement: The host's room is measured

While its telemetry sends metrics, sluice's host process SHALL send
every three seconds, for each repository and branch with a run counted
against the bound then or at its last sending, `sluice.host.running`,
its runs, and `sluice.host.bound`, the bound, reading 0 once after its
last run; and for each run whose part it held, `sluice.run.waited_for_room`,
the seconds from the ask first refused to the hold, 0 where none was
(71, 150, 154).

#### Scenario: A run's wait is timed by branch

- **WHEN** a combination of `main` is refused at 0 s and held at 20 s,
  one of `bolt/x` of the same repository is held at its first ask, and
  a session's run waits from 5 s and is held at 12 s
- **THEN** the room gives 20 s for `main`, 0 s for `bolt/x` and 7 s for
  the session's branch, once each
  (`a_run_s_wait_for_room_is_measured_by_branch` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: The room is sent by branch

- **WHEN** the room counts two runs for `main` with a bound of 4, then
  none, and a wait of 20 s was drained
- **THEN** the first sending reports `sluice.host.running` 2 and
  `sluice.host.bound` 4 for `main` and `sluice.run.waited_for_room` 20,
  the second `sluice.host.running` 0 for `main`, and the third nothing
  for it, each with the repository and the branch
  (`the_host_s_room_is_measured_by_branch` in
  `crates/sluice/src/host_process/mod.rs`)

### Requirement: A submission's times and a run's wait are sent in seconds

`sluice.submission.waited`, `sluice.submission.tested` and
`sluice.run.waited_for_room` SHALL be declared in seconds, the unit
`s`; `sluice.submissions.ended`, `sluice.host.running` and
`sluice.host.bound` SHALL declare no unit (rule core.66).

#### Scenario: Each is declared as it measures

- **WHEN** sluice's meter is sent each of the six as the core and the
  host process emit them
- **THEN** the first three are exported with the unit `s` and the
  others with none (`a_submission_s_times_are_sent_in_seconds` in
  `crates/sluice/src/telemetry.rs`)
