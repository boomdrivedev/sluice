//! OpenTelemetry for sluice (ADR 0014): off unless the host's own
//! settings, `[telemetry]`, or a command's own `--otel` or `SLUICE_OTEL`
//! turn it on, then the core's `tracing` spans and metric events exported
//! over OTLP/HTTP to the table's collector, else wherever the standard
//! `OTEL_EXPORTER_OTLP_*` variables say (151; record 0106; rule core.53).
//! A command's own choice is for that command alone, over the table
//! ([`Settings::choose`]). Telemetry decides nothing: an exporter that
//! fails is silent, and nothing reads it back.
//!
//! OpenTelemetry's own settings turn it, or one signal, off again, as its
//! SDK environment variables specification reads them: `OTEL_SDK_DISABLED`
//! at `true` every signal, and `OTEL_TRACES_EXPORTER`,
//! `OTEL_METRICS_EXPORTER` or `OTEL_LOGS_EXPORTER` at `none` that one
//! alone; they never turn on a signal the table turns off. A signal
//! turned off is never built.
//!
//! A run's checks get what sluice sends: its collector and the signals it
//! sends, its resource attributes and, in `OTEL_RESOURCE_ATTRIBUTES`, the
//! trace and span of the round that started them, so what they export is
//! found beside the round; and the W3C `TRACEPARENT`, for a tool that
//! reads it to nest its spans under the start.
//!
//! Logs (ADR 0022): a failed check's output, which the core emits once
//! where it keeps it, and sluice's own warnings and errors, sent as OTLP
//! log records under the span they were emitted in. The host's `[telemetry]
//! logs`, or a command's `SLUICE_OTEL_LOGS`, says how much of the output
//! goes: none, a summary naming where it is kept, or its end scrubbed of
//! what reads as a secret. The kept copy on disk is the record (20, 26;
//! rule core.11); a record lost on the way is never sent again.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use opentelemetry::logs::{AnyValue, LogRecord as _};
use opentelemetry::trace::{TraceContextExt, TracerProvider as _};
use opentelemetry::InstrumentationScope;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{self, LogProcessor, SdkLogRecord, SdkLoggerProvider};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::{Instrument, SdkMeterProvider, Stream};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use sluice_core::{OUTPUT, SECONDS};
use sluice_seams::LOG_LIMIT;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetrySpanExt};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

use crate::instance::HostTelemetry;

static ON: AtomicBool = AtomicBool::new(false);

/// The settings telemetry started under, which a run's environment is
/// built from (design D6).
static SENT: OnceLock<Settings> = OnceLock::new();

/// How much of a failed check's output is sent (ADR 0022): the flag and
/// the host's `[telemetry] logs` read one set of words (design D1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Logs {
    /// No logs at all.
    Off,
    /// Which task failed and where its output is kept; never the output.
    Summary,
    /// The summary's fields and the output's end, scrubbed.
    Tail,
}

/// Which signals OpenTelemetry's own settings leave on, as its SDK
/// environment variables specification reads them (ADR 0014):
/// `OTEL_SDK_DISABLED` at `true` turns every signal off, and
/// `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` or `OTEL_LOGS_EXPORTER`
/// at `none` turns that one alone off, each value read trimmed and without
/// its ASCII case. Any other value, or none, leaves a signal on: the
/// environment only turns a signal off, never on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Signals {
    traces: bool,
    metrics: bool,
    logs: bool,
}

impl Signals {
    /// The signals `env`, as name and value pairs, leaves on.
    fn read(env: impl Iterator<Item = (String, String)>) -> Signals {
        let mut on = Signals {
            traces: true,
            metrics: true,
            logs: true,
        };
        let mut disabled = false;
        for (name, value) in env {
            let is = |word: &str| value.trim().eq_ignore_ascii_case(word);
            match name.as_str() {
                "OTEL_SDK_DISABLED" => disabled = is("true"),
                "OTEL_TRACES_EXPORTER" => on.traces = !is("none"),
                "OTEL_METRICS_EXPORTER" => on.metrics = !is("none"),
                "OTEL_LOGS_EXPORTER" => on.logs = !is("none"),
                _ => {}
            }
        }
        if disabled {
            return Signals {
                traces: false,
                metrics: false,
                logs: false,
            };
        }
        on
    }
}

/// The service sluice reports as where neither the command nor the host's
/// table names one.
pub const SERVICE: &str = "sluice";

/// The most of a failed check's output sent at `tail`, in bytes, where
/// neither the command nor the host's table says.
pub const TAIL: usize = 8192;

/// What a command was given of telemetry: its own flag, else its
/// `SLUICE_OTEL*` variable, each `None` where it was given neither, so
/// the host's `[telemetry]` holds (151; design D2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    /// `--otel`, `SLUICE_OTEL`.
    pub otel: Option<bool>,
    /// `--otel-service-name`, `SLUICE_OTEL_SERVICE_NAME`.
    pub service: Option<String>,
    /// `--otel-logs`, `SLUICE_OTEL_LOGS`.
    pub logs: Option<Logs>,
    /// `--otel-logs-limit`, `SLUICE_OTEL_LOGS_LIMIT`, in bytes.
    pub logs_limit: Option<usize>,
}

/// The telemetry a command sends, chosen once as it starts (151; record
/// 0106; rule core.53): each setting the command's own choice, else the
/// host's `[telemetry]`, else sluice's default; the signals both the
/// table and OpenTelemetry's variables leave on; and the environment as
/// it was when chosen, which everything sluice reads of it is read from
/// (design D2 to D6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Whether telemetry is asked for.
    pub send: bool,
    /// The service sluice reports as.
    pub service: String,
    /// How much of a failed check's output is sent.
    pub logs: Logs,
    /// At `tail`, the most of a failed check's output sent, in bytes.
    pub limit: usize,
    /// The table's collector, an `http://` address, where it names one.
    collector: Option<String>,
    /// The table's `attributes`; `None` where the host has no table.
    table: Option<BTreeMap<String, String>>,
    /// The host's own name.
    host: String,
    /// The signals the table and OpenTelemetry's variables leave on.
    signals: Signals,
    /// The environment as it was when chosen (design D4).
    env: Vec<(String, String)>,
}

/// The resource sluice sends as (design D5): the attributes it names
/// itself, and whether OpenTelemetry's `OTEL_RESOURCE_ATTRIBUTES` and
/// `OTEL_SERVICE_NAME` are read beside them, as they are only where the
/// host has no `[telemetry]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub pairs: Vec<(String, String)>,
    pub reads_environment: bool,
}

impl Settings {
    /// The settings `choices` and the host's `table` give on the host
    /// named `host`, with `env`, as name and value pairs, the environment
    /// they keep and read: each setting the command's choice, else the
    /// table's, else sluice's default, a command's tail held to 65536
    /// bytes (design D2). A signal is on only while the table and
    /// OpenTelemetry's variables both leave it on, and logs only at a
    /// level other than `off` (design D3).
    pub fn choose(
        choices: Choices,
        table: Option<&HostTelemetry>,
        host: &str,
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Settings {
        let env: Vec<(String, String)> = env.into_iter().collect();
        let send = choices
            .otel
            .or_else(|| table.and_then(|t| t.send))
            .unwrap_or(false);
        let service = choices
            .service
            .or_else(|| table.and_then(|t| t.service.clone()))
            .unwrap_or_else(|| SERVICE.to_string());
        let logs = choices
            .logs
            .or_else(|| table.and_then(|t| t.logs))
            .unwrap_or(Logs::Summary);
        let limit = choices
            .logs_limit
            .map(|n| n.min(LOG_LIMIT))
            .or_else(|| table.and_then(|t| t.logs_tail_bytes))
            .unwrap_or(TAIL);
        let left = Signals::read(env.iter().cloned());
        let signals = Signals {
            traces: left.traces && table.and_then(|t| t.traces).unwrap_or(true),
            metrics: left.metrics && table.and_then(|t| t.metrics).unwrap_or(true),
            logs: left.logs && logs != Logs::Off,
        };
        Settings {
            send,
            service,
            logs,
            limit,
            collector: table.and_then(|t| t.collector.clone()),
            table: table.map(|t| t.attributes.clone()),
            host: host.to_string(),
            signals,
            env,
        }
    }

    /// The resource sluice sends as (design D5). With a table:
    /// `service.name`, `service.version`, `host.name` unless the table's
    /// `attributes` names one, and those, the environment's read not at
    /// all. With none: `service.name`, `service.version` and `host.name`
    /// unless `OTEL_RESOURCE_ATTRIBUTES` names one, the environment's
    /// read beside them as OpenTelemetry reads them.
    pub fn resource(&self) -> Named {
        let mut pairs = vec![
            ("service.name".to_string(), self.service.clone()),
            ("service.version".to_string(), crate::VERSION.to_string()),
        ];
        pairs.extend(self.named());
        Named {
            pairs,
            reads_environment: self.table.is_none(),
        }
    }

    /// The attributes sluice names its host and the table's with:
    /// `host.name`, the host's own name, unless the table's `attributes`,
    /// or with no table `OTEL_RESOURCE_ATTRIBUTES`, names one; then the
    /// table's `attributes` (design D5).
    fn named(&self) -> Vec<(String, String)> {
        let names_host = match &self.table {
            Some(attributes) => attributes.contains_key("host.name"),
            None => self.var("OTEL_RESOURCE_ATTRIBUTES").is_some_and(|v| {
                v.split(',')
                    .any(|pair| pair.split('=').next().map(str::trim) == Some("host.name"))
            }),
        };
        let mut pairs = Vec::new();
        if !names_host {
            pairs.push(("host.name".to_string(), self.host.clone()));
        }
        if let Some(attributes) = &self.table {
            pairs.extend(attributes.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        pairs
    }

    /// `builder`, an exporter's of `signal`, sending to the table's
    /// collector, at its `/v1/<signal>`, protobuf, where it names one; as
    /// it is, reading the environment, where it names none (design D4).
    fn to<B: WithExportConfig>(&self, builder: B, signal: &str) -> B {
        match &self.collector {
            Some(collector) => builder
                .with_endpoint(format!("{collector}/v1/{signal}"))
                .with_protocol(Protocol::HttpBinary),
            None => builder,
        }
    }

    /// The table's collector, where it names one: sluice's alone (design
    /// D4).
    pub fn collector(&self) -> Option<&str> {
        self.collector.as_deref()
    }

    /// The value the environment, as chosen, gives `name`.
    fn var(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The meter provider sluice's metrics are sent through, to `exporter`
/// with `resource`: each instrument [`SECONDS`] names declared with the
/// unit `s`, OpenTelemetry's own name for seconds, by a view, since the
/// core's metric events are `tracing`'s and name no unit (rule core.66;
/// record 0014 as amended 2026-11-18; design D4).
pub fn meter_provider<E: PushMetricExporter>(exporter: E, resource: Resource) -> SdkMeterProvider {
    SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(resource)
        .with_view(|i: &Instrument| {
            SECONDS
                .contains(&i.name())
                .then(|| Stream::builder().with_unit("s").build().ok())
                .flatten()
        })
        .build()
}

/// The exporters, flushed by [`Telemetry::finish`] before sluice exits:
/// a provider for each signal left on, and none for a signal turned off.
pub struct Telemetry {
    traces: Option<SdkTracerProvider>,
    metrics: Option<SdkMeterProvider>,
    logs: Option<SdkLoggerProvider>,
}

impl Telemetry {
    /// Export from here on as `settings` say: each signal only while they
    /// leave it on, as the service they name, with the resource they give
    /// and as much of a failure's output as they ask (design D3, D5).
    /// Where the host's table names a collector, every exporter sends
    /// there, protobuf, and to nowhere else: the caller's
    /// `OTEL_EXPORTER_OTLP_*` are first taken out of this process's
    /// environment, which the exporters would otherwise read for their
    /// headers, timeout and compression. `main` starts telemetry before
    /// any thread of sluice's runs; what sluice itself reads of the
    /// environment is read from the settings' snapshot (design D4).
    pub fn start(settings: &Settings) -> Result<Telemetry> {
        if settings.collector.is_some() {
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("OTEL_EXPORTER_OTLP_") {
                    std::env::remove_var(name);
                }
            }
        }
        let spans = match settings.signals.traces {
            true => Some(
                settings
                    .to(SpanExporter::builder().with_http(), "traces")
                    .build()?,
            ),
            false => None,
        };
        let exporter = match settings.signals.logs {
            true => Some(
                settings
                    .to(LogExporter::builder().with_http(), "logs")
                    .build()?,
            ),
            false => None,
        };
        let metrics = match settings.signals.metrics {
            true => Some(
                settings
                    .to(MetricExporter::builder().with_http(), "metrics")
                    .build()?,
            ),
            false => None,
        };
        Telemetry::begin(settings, spans, metrics, exporter)
    }

    /// [`Telemetry::start`], with the spans sent to `spans` and the logs
    /// to `exporter`, each dropped where its signal is off.
    pub fn start_with<S, E>(settings: &Settings, spans: S, exporter: Option<E>) -> Result<Telemetry>
    where
        S: opentelemetry_sdk::trace::SpanExporter + 'static,
        E: logs::LogExporter + 'static,
    {
        let metrics = match settings.signals.metrics {
            true => Some(
                settings
                    .to(MetricExporter::builder().with_http(), "metrics")
                    .build()?,
            ),
            false => None,
        };
        Telemetry::begin(settings, Some(spans), metrics, exporter)
    }

    /// The providers of the signals `settings` leave on, and a subscriber
    /// that layers only theirs: a signal turned off is never built, so
    /// nothing of it is made or sent. With traces off no span has a
    /// trace, so a run gets no `TRACEPARENT` and a failure's record names
    /// no span. With none left on, telemetry is off as if never asked
    /// for: no subscriber, [`on`] false and nothing said, since the host
    /// turned it off itself.
    fn begin<S, E>(
        settings: &Settings,
        spans: Option<S>,
        metrics: Option<MetricExporter>,
        exporter: Option<E>,
    ) -> Result<Telemetry>
    where
        S: opentelemetry_sdk::trace::SpanExporter + 'static,
        E: logs::LogExporter + 'static,
    {
        let on = settings.signals;
        let spans = spans.filter(|_| on.traces);
        let metrics = metrics.filter(|_| on.metrics);
        let exporter = exporter.filter(|_| on.logs);
        if spans.is_none() && metrics.is_none() && exporter.is_none() {
            return Ok(Telemetry {
                traces: None,
                metrics: None,
                logs: None,
            });
        }
        let named = settings.resource();
        let resource = match named.reads_environment {
            true => Resource::builder(),
            false => Resource::builder_empty(),
        }
        .with_attributes(
            named
                .pairs
                .into_iter()
                .map(|(k, v)| opentelemetry::KeyValue::new(k, v)),
        )
        .build();
        let traces = spans.map(|spans| {
            SdkTracerProvider::builder()
                .with_batch_exporter(spans)
                .with_resource(resource.clone())
                .build()
        });
        let metrics = metrics.map(|metrics| meter_provider(metrics, resource.clone()));
        let logs = exporter.map(|e| {
            SdkLoggerProvider::builder()
                .with_log_processor(Scrub::new(
                    settings.logs,
                    settings.limit,
                    settings.env.iter().cloned(),
                ))
                .with_batch_exporter(e)
                .with_resource(resource)
                .build()
        });
        // Only a failure's output and what sluice warns of become logs:
        // never a metric's event.
        let bridge = logs.as_ref().map(|l| {
            OpenTelemetryTracingBridge::new(l).with_filter(filter_fn(|m| {
                m.is_event() && (m.target() == OUTPUT || *m.level() <= tracing::Level::WARN)
            }))
        });
        let subscriber = tracing_subscriber::registry()
            .with(
                traces
                    .as_ref()
                    .map(|t| tracing_opentelemetry::layer().with_tracer(t.tracer("sluice"))),
            )
            .with(metrics.clone().map(MetricsLayer::new))
            .with(bridge);
        tracing::subscriber::set_global_default(subscriber)?;
        let _ = SENT.set(settings.clone());
        ON.store(true, Ordering::SeqCst);
        Ok(Telemetry {
            traces,
            metrics,
            logs,
        })
    }

    /// Send what is held so far, every signal's, and go on: what a
    /// `sluice run` holds is sent where it was sending before it starts
    /// afresh (135; design D5 of `a-kept-run-hands-over-what-it-cannot-take-up`).
    pub fn flush(&self) {
        if let Some(t) = &self.traces {
            let _ = t.force_flush();
        }
        if let Some(m) = &self.metrics {
            let _ = m.force_flush();
        }
        if let Some(l) = &self.logs {
            let _ = l.force_flush();
        }
    }

    /// Send what is left. A collector that cannot be reached loses it.
    pub fn finish(self) {
        if let Some(t) = self.traces {
            let _ = t.shutdown();
        }
        if let Some(m) = self.metrics {
            let _ = m.shutdown();
        }
        if let Some(l) = self.logs {
            let _ = l.shutdown();
        }
    }
}

/// A line sluice says on stderr, sent as a warning while telemetry sends
/// logs.
pub fn warn(line: &str) {
    eprintln!("{line}");
    tracing::warn!("{line}");
}

/// A line sluice says on stderr, sent as an error while telemetry sends
/// logs.
pub fn error(line: &str) {
    eprintln!("{line}");
    tracing::error!("{line}");
}

/// What a log record's body is made before it is sent: a failure's output
/// as the level says, and every body scrubbed of what reads as a secret.
#[derive(Debug)]
struct Scrub {
    level: Logs,
    limit: usize,
    secrets: crate::scrub::Scrub,
}

impl Scrub {
    fn new(level: Logs, limit: usize, env: impl Iterator<Item = (String, String)>) -> Scrub {
        Scrub {
            level,
            limit: limit.min(LOG_LIMIT),
            secrets: crate::scrub::Scrub::new(env),
        }
    }

    fn scrub(&self, text: &str) -> String {
        self.secrets.scrub(text)
    }

    /// The body a record of `target` with `body` is sent with.
    fn body(&self, target: &str, body: &str, attr: impl Fn(&str) -> Option<String>) -> String {
        if target != OUTPUT {
            return self.scrub(body);
        }
        match self.level {
            Logs::Tail => end(&self.scrub(body), self.limit).to_string(),
            _ => {
                let what = attr("sluice.task")
                    .or_else(|| attr("sluice.preview").map(|p| format!("preview {p}")))
                    .unwrap_or_else(|| "a check".into());
                match attr("sluice.kept").filter(|k| !k.is_empty()) {
                    Some(at) => format!("{what} failed; its output is kept at {at}"),
                    None => format!("{what} failed"),
                }
            }
        }
    }
}

impl LogProcessor for Scrub {
    fn emit(&self, record: &mut SdkLogRecord, _: &InstrumentationScope) {
        let Some(AnyValue::String(body)) = record.body() else {
            return;
        };
        let target = record.target().map(|t| t.to_string()).unwrap_or_default();
        let attr = |key: &str| {
            record
                .attributes_iter()
                .find(|(k, _)| k.as_str() == key)
                .map(|(_, v)| match v {
                    AnyValue::String(s) => s.as_str().to_string(),
                    other => format!("{other:?}"),
                })
        };
        let body = self.body(&target, body.as_str(), attr);
        record.set_body(AnyValue::String(body.into()));
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
}

use crate::scrub::end;

/// Whether this process exports.
pub fn on() -> bool {
    ON.load(Ordering::SeqCst)
}

/// What a run's environment takes of telemetry: nothing when it is off;
/// otherwise what sluice sends, built from the settings it started under,
/// never from this process's environment as it is now (design D6).
pub fn run_env(attributes: &[(&str, &str)]) -> BTreeMap<String, String> {
    let Some(settings) = SENT.get().filter(|_| on()) else {
        return BTreeMap::new();
    };
    let cx = tracing::Span::current().context();
    let span = cx.span();
    variables(settings, span.span_context(), attributes)
}

/// A run's telemetry under `settings`, started under the span `ids`, with
/// the run's own `attributes` (design D6). With a table: the collector
/// sluice sends to, the table's or else the caller's
/// `OTEL_EXPORTER_OTLP_*`; `OTEL_<SIGNAL>_EXPORTER=none` for each signal
/// sluice does not send; and `OTEL_RESOURCE_ATTRIBUTES` holding sluice's
/// `host.name` and the table's `attributes`, never `service.name`, so a
/// tool reports as itself; no other `OTEL_*`. With none: the caller's
/// `OTEL_*` as they were when chosen, `host.name` added where sluice
/// named the host itself. Either way, while traces go, the trace and span
/// ids and `TRACEPARENT`, and then the run's own attributes, each value
/// encoded.
fn variables(
    settings: &Settings,
    ids: &opentelemetry::trace::SpanContext,
    attributes: &[(&str, &str)],
) -> BTreeMap<String, String> {
    let caller = |prefix: &'static str| {
        settings
            .env
            .iter()
            .filter(move |(k, _)| k.starts_with(prefix))
            .cloned()
    };
    let named = settings
        .named()
        .into_iter()
        .map(|(k, v)| format!("{k}={}", encode(&v)));
    let (mut env, mut pairs): (BTreeMap<String, String>, Vec<String>) = match &settings.table {
        None => {
            let mut env: BTreeMap<String, String> = caller("OTEL_").collect();
            let pairs = env
                .remove("OTEL_RESOURCE_ATTRIBUTES")
                .into_iter()
                .filter(|s| !s.is_empty())
                .chain(named)
                .collect();
            (env, pairs)
        }
        Some(_) => {
            let mut env: BTreeMap<String, String> = match &settings.collector {
                Some(collector) => BTreeMap::from([
                    ("OTEL_EXPORTER_OTLP_ENDPOINT".into(), collector.clone()),
                    ("OTEL_EXPORTER_OTLP_PROTOCOL".into(), "http/protobuf".into()),
                ]),
                None => caller("OTEL_EXPORTER_OTLP_").collect(),
            };
            let on = settings.signals;
            for (sent, variable) in [
                (on.traces, "OTEL_TRACES_EXPORTER"),
                (on.metrics, "OTEL_METRICS_EXPORTER"),
                (on.logs, "OTEL_LOGS_EXPORTER"),
            ] {
                if !sent {
                    env.insert(variable.into(), "none".into());
                }
            }
            (env, named.collect())
        }
    };
    if ids.is_valid() {
        let (trace, span) = (ids.trace_id().to_string(), ids.span_id().to_string());
        pairs.push(format!("sluice.trace_id={trace}"));
        pairs.push(format!("sluice.span_id={span}"));
        env.insert(
            "TRACEPARENT".into(),
            format!("00-{trace}-{span}-{:02x}", ids.trace_flags().to_u8()),
        );
    }
    for (k, v) in attributes {
        pairs.push(format!("{k}={}", encode(v)));
    }
    env.insert("OTEL_RESOURCE_ATTRIBUTES".into(), pairs.join(","));
    env
}

/// A resource attribute's value, percent-encoded where the variable's
/// own syntax would read it (W3C baggage octets).
fn encode(v: &str) -> String {
    let mut s = String::new();
    for b in v.bytes() {
        match b {
            b',' | b'=' | b';' | b'%' | b'"' | b'\\' | 0..=0x20 | 0x7f.. => {
                s.push_str(&format!("%{b:02X}"))
            }
            _ => s.push(b as char),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule core.66 (design D4): every time the core gives in seconds is
    /// sent declared in `s`, and a count with no unit.
    #[test]
    fn a_time_is_sent_in_seconds() {
        use opentelemetry_sdk::metrics::InMemoryMetricExporter;
        let exporter = InMemoryMetricExporter::default();
        let provider = meter_provider(exporter.clone(), Resource::builder_empty().build());
        let subscriber = tracing_subscriber::registry().with(MetricsLayer::new(provider.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let (repository, branch) = ("acme/app", "main");
            tracing::info!(
                histogram.sluice.submission.landed_after = 3.5f64,
                sluice.repository = repository,
                sluice.branch = branch,
            );
            tracing::info!(
                histogram.sluice.candidate.tested_for = 2.0f64,
                sluice.repository = repository,
                sluice.branch = branch,
                sluice.outcome = "passed",
            );
            tracing::info!(
                monotonic_counter.sluice.tasks.time_saved = 4.0f64,
                sluice.repository = repository,
                sluice.branch = branch,
            );
            tracing::info!(
                monotonic_counter.sluice.landed = 1u64,
                sluice.repository = repository,
                sluice.branch = branch,
            );
        });
        provider.force_flush().unwrap();
        let mut units: Vec<(String, String)> = exporter
            .get_finished_metrics()
            .unwrap()
            .iter()
            .flat_map(|r| r.scope_metrics())
            .flat_map(|s| s.metrics())
            .map(|m| (m.name().to_string(), m.unit().to_string()))
            .collect();
        units.sort();
        units.dedup();
        assert_eq!(
            units,
            [
                ("sluice.candidate.tested_for".to_string(), "s".to_string()),
                ("sluice.landed".to_string(), String::new()),
                (
                    "sluice.submission.landed_after".to_string(),
                    "s".to_string()
                ),
                ("sluice.tasks.time_saved".to_string(), "s".to_string()),
            ]
        );
    }

    #[test]
    fn values_are_encoded_where_the_syntax_would_read_them() {
        assert_eq!(encode("sluice/try/bolt/x/3"), "sluice/try/bolt/x/3");
        assert_eq!(encode("a,b=c d"), "a%2Cb%3Dc%20d");
    }

    fn scrub(level: Logs, limit: usize, env: &[(&str, &str)]) -> Scrub {
        let env = env.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        Scrub::new(level, limit, env.collect::<Vec<_>>().into_iter())
    }

    #[test]
    fn what_reads_as_a_secret_is_redacted() {
        let s = scrub(Logs::Tail, 4096, &[]);
        for secret in [
            "ghp_0123456789abcdefghijABCDEFGHIJ012345",
            "github_pat_11ABCDEFG0123456789_abcdefghijkl",
            "AKIAABCDEFGHIJKLMNOP",
            "xoxb-1234567890-abcdef",
            "sk-ant-api03-abcdefghijkl",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJl",
            "Bearer abcdefgh.ijkl",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----",
        ] {
            let out = s.body(OUTPUT, &format!("before {secret} after"), |_| None);
            assert_eq!(out, "before [redacted] after", "{secret}");
        }
        assert_eq!(s.body(OUTPUT, "ok: 3 passed", |_| None), "ok: 3 passed");
    }

    #[test]
    fn a_secret_variable_s_value_is_redacted_wherever_it_appears() {
        let s = scrub(
            Logs::Tail,
            4096,
            &[
                ("NPM_TOKEN", "s3cr3t-value-1"),
                ("OTEL_EXPORTER_OTLP_HEADERS", "x-api=abcdefgh"),
                ("HOME", "/home/someone"),
                ("API_KEY", "short"),
            ],
        );
        let out = s.body(
            OUTPUT,
            "a s3cr3t-value-1 b x-api=abcdefgh c /home/someone short",
            |_| None,
        );
        assert_eq!(out, "a [redacted] b [redacted] c /home/someone short");
    }

    #[test]
    fn a_summary_sends_where_the_output_is_kept_never_the_output() {
        let s = scrub(Logs::Summary, 4096, &[]);
        let attr = |k: &str| match k {
            "sluice.task" => Some("a:test".to_string()),
            "sluice.kept" => Some("/x/.logs/a.log".to_string()),
            _ => None,
        };
        assert_eq!(
            s.body(OUTPUT, "the output", attr),
            "a:test failed; its output is kept at /x/.logs/a.log"
        );
        // What sluice warns of is never summarised, only scrubbed.
        assert_eq!(
            s.body(
                "sluice::lib",
                "a warning ghp_0123456789abcdefghijABCDEF",
                |_| None
            ),
            "a warning [redacted]"
        );
    }

    #[test]
    fn a_tail_keeps_the_end_from_a_character_s_start() {
        // é is two bytes: the last two of "abcdéf" begin inside it.
        let s = scrub(Logs::Tail, 2, &[]);
        assert_eq!(s.body(OUTPUT, "abcdéf", |_| None), "f");
        assert_eq!(s.body(OUTPUT, "abcdefgh", |_| None), "gh");
        assert_eq!(scrub(Logs::Tail, 1 << 30, &[]).limit, LOG_LIMIT);
    }

    fn signals(env: &[(&str, &str)]) -> Signals {
        let env = env.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        Signals::read(env.collect::<Vec<_>>().into_iter())
    }

    #[test]
    fn the_standard_settings_are_read_as_opentelemetry_reads_them() {
        let all = Signals {
            traces: true,
            metrics: true,
            logs: true,
        };
        let none = Signals {
            traces: false,
            metrics: false,
            logs: false,
        };
        assert_eq!(signals(&[]), all);
        for value in ["none", "NONE", " None "] {
            let only = Signals {
                traces: false,
                ..all
            };
            assert_eq!(
                signals(&[("OTEL_TRACES_EXPORTER", value)]),
                only,
                "{value:?}"
            );
        }
        for value in ["otlp", "console"] {
            assert_eq!(
                signals(&[("OTEL_TRACES_EXPORTER", value)]),
                all,
                "{value:?}"
            );
        }
        for value in ["true", " TRUE "] {
            assert_eq!(signals(&[("OTEL_SDK_DISABLED", value)]), none, "{value:?}");
        }
        for value in ["1", "false", ""] {
            assert_eq!(signals(&[("OTEL_SDK_DISABLED", value)]), all, "{value:?}");
        }
        assert_eq!(
            signals(&[("OTEL_METRICS_EXPORTER", "none")]),
            Signals {
                metrics: false,
                ..all
            }
        );
        assert_eq!(
            signals(&[("OTEL_LOGS_EXPORTER", "none")]),
            Signals { logs: false, ..all }
        );
    }

    fn pairs(env: &[(&str, &str)]) -> Vec<(String, String)> {
        env.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn choose(choices: Choices, table: Option<&HostTelemetry>, env: &[(&str, &str)]) -> Settings {
        Settings::choose(choices, table, "host-a", pairs(env))
    }

    /// 151: each setting the command's choice, else the table's, else
    /// sluice's default; `SLUICE_OTEL=0` turns a table's `send = true` off
    /// for the command, and `--otel` turns telemetry on where the table
    /// does not (record 0106; design D2).
    #[test]
    fn the_settings_are_chosen_by_the_command_then_the_table() {
        let none = Choices::default();
        let otel = |on| Choices {
            otel: Some(on),
            ..Choices::default()
        };
        let sends = HostTelemetry {
            send: Some(true),
            collector: Some("http://localhost:4318".into()),
            ..HostTelemetry::default()
        };
        let quiet = HostTelemetry {
            send: Some(false),
            ..HostTelemetry::default()
        };
        assert!(!choose(none.clone(), None, &[]).send);
        assert!(!choose(none.clone(), Some(&HostTelemetry::default()), &[]).send);
        assert!(choose(none.clone(), Some(&sends), &[]).send);
        assert!(!choose(otel(false), Some(&sends), &[]).send);
        assert!(choose(otel(true), None, &[]).send);
        assert!(choose(otel(true), Some(&quiet), &[]).send);

        let defaults = choose(none.clone(), None, &[]);
        assert_eq!(
            (defaults.service.as_str(), defaults.logs, defaults.limit),
            ("sluice", Logs::Summary, 8192)
        );
        let table = HostTelemetry {
            service: Some("sluice-a".into()),
            logs: Some(Logs::Tail),
            logs_tail_bytes: Some(4096),
            ..HostTelemetry::default()
        };
        let tables = choose(none.clone(), Some(&table), &[]);
        assert_eq!(
            (tables.service.as_str(), tables.logs, tables.limit),
            ("sluice-a", Logs::Tail, 4096)
        );
        let commands = Choices {
            otel: None,
            service: Some("ci-sluice".into()),
            logs: Some(Logs::Off),
            logs_limit: Some(100),
        };
        for table in [None, Some(&table)] {
            let chosen = choose(commands.clone(), table, &[]);
            assert_eq!(
                (chosen.service.as_str(), chosen.logs, chosen.limit),
                ("ci-sluice", Logs::Off, 100)
            );
        }
        let over = Choices {
            logs_limit: Some(1 << 30),
            ..Choices::default()
        };
        assert_eq!(choose(over, Some(&table), &[]).limit, LOG_LIMIT);
    }

    /// 151: a signal goes only while the table and OpenTelemetry's
    /// variables both leave it on; the environment never turns on what
    /// the table turns off (record 0106; design D3).
    #[test]
    fn the_table_turns_a_signal_off_and_the_environment_never_turns_it_on() {
        let table = |traces, metrics, logs| HostTelemetry {
            send: Some(true),
            collector: Some("http://localhost:4318".into()),
            traces,
            metrics,
            logs,
            ..HostTelemetry::default()
        };
        let signals = |t: HostTelemetry, env: &[(&str, &str)]| {
            choose(Choices::default(), Some(&t), env).signals
        };
        let off = signals(
            table(Some(false), None, None),
            &[("OTEL_TRACES_EXPORTER", "otlp")],
        );
        assert!(!off.traces && off.metrics && off.logs, "{off:?}");
        let off = signals(
            table(Some(true), None, None),
            &[("OTEL_TRACES_EXPORTER", "none")],
        );
        assert!(!off.traces && off.metrics && off.logs, "{off:?}");
        let on = signals(table(None, None, None), &[]);
        assert!(on.traces && on.metrics && on.logs, "{on:?}");
        let off = signals(table(None, Some(false), None), &[]);
        assert!(off.traces && !off.metrics, "{off:?}");
        for env in [
            &[][..],
            &[("OTEL_LOGS_EXPORTER", "otlp")][..],
            &[("OTEL_LOGS_EXPORTER", "none")][..],
        ] {
            let off = signals(table(None, None, Some(Logs::Off)), env);
            assert!(off.traces && off.metrics && !off.logs, "{env:?}: {off:?}");
        }
        // A command's level of `off` turns logs off too, and `none` turns
        // them off at any level.
        let commands = Choices {
            logs: Some(Logs::Off),
            ..Choices::default()
        };
        assert!(!choose(commands, None, &[]).signals.logs);
        let off = choose(Choices::default(), None, &[("OTEL_LOGS_EXPORTER", "none")]);
        assert!(!off.signals.logs);
        let all = choose(Choices::default(), None, &[("OTEL_SDK_DISABLED", "true")]);
        assert_eq!(
            all.signals,
            Signals {
                traces: false,
                metrics: false,
                logs: false
            }
        );
    }

    /// 151: with a table the resource is sluice's own, `host.name` the
    /// host's unless the table's `attributes` names one, and no variable
    /// is read; with none, `OTEL_RESOURCE_ATTRIBUTES` is read beside it,
    /// and sluice names the host unless that does (record 0106; design
    /// D5).
    #[test]
    fn the_resource_is_the_tables_and_names_the_host() {
        let named = |table: Option<&HostTelemetry>, env: &[(&str, &str)]| {
            choose(Choices::default(), table, env).resource()
        };
        let own = |rest: &[(&str, &str)]| {
            let mut all = pairs(&[
                ("service.name", "sluice-a"),
                ("service.version", crate::VERSION),
            ]);
            all.extend(pairs(rest));
            all
        };
        let table = |attributes: &[(&str, &str)]| HostTelemetry {
            service: Some("sluice-a".into()),
            attributes: pairs(attributes).into_iter().collect(),
            ..HostTelemetry::default()
        };
        let session = [("OTEL_RESOURCE_ATTRIBUTES", "deployment.environment=session")];

        let prod = table(&[("deployment.environment", "prod")]);
        assert_eq!(
            named(Some(&prod), &session),
            Named {
                pairs: own(&[("host.name", "host-a"), ("deployment.environment", "prod")]),
                reads_environment: false,
            }
        );
        let boxed = table(&[("host.name", "box")]);
        assert_eq!(
            named(Some(&boxed), &[]),
            Named {
                pairs: own(&[("host.name", "box")]),
                reads_environment: false,
            }
        );
        let service = |all: Vec<(String, String)>| {
            all.into_iter()
                .map(|(k, v)| match k.as_str() {
                    "service.name" => (k, "sluice".to_string()),
                    _ => (k, v),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            named(None, &session),
            Named {
                pairs: service(own(&[("host.name", "host-a")])),
                reads_environment: true,
            }
        );
        assert_eq!(
            named(
                None,
                &[(
                    "OTEL_RESOURCE_ATTRIBUTES",
                    "deployment.environment=session, host.name=box"
                )]
            ),
            Named {
                pairs: service(own(&[])),
                reads_environment: true,
            }
        );
    }

    #[test]
    fn a_run_gets_nothing_while_telemetry_is_off() {
        assert!(run_env(&[("sluice.place", "p")]).is_empty());
    }
}
