//! What sluice sends through the exporters it sets up (ADRs 0014, 0022):
//! a failure's output as an OTLP log record under the span of the round
//! that read the verdict, scrubbed, as much of it as the host asks and
//! none at `off`; a failed preview's, and sluice's warnings; a round's
//! spans, how they nest and what they carry; a run's checks joined to the
//! round; OTLP over HTTP to the collector, whose absence fails
//! nothing; and each signal sent only while OpenTelemetry's own settings
//! leave it on. Each test runs alone in a process of its own, since the
//! exporters' subscriber is the process's.

mod support;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use opentelemetry::logs::{AnyValue, Severity};
use opentelemetry::trace::{SpanId, TraceId};
use opentelemetry::Value;
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLogRecord};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SpanData};
use sluice::instance::{HostTelemetry, Instance};
use sluice::profiles::checks::Checks;
use sluice::telemetry::{Caller, Choices, Logs, Settings as Chosen, Telemetry};
use sluice::{run, Cli, Followed, Opened, Pause, PullRequests, FAILED, LANDED};
use sluice_core::{Clock, Seams, SubmitHead, SubmitRefusal, OUTPUT};
use sluice_fakes::Fakes;
use sluice_seams::{Limits, Outcome, Place, Progress, Rev, RunRequest, Setting, Settings, Tests};

const BRANCH: &str = "bolt/telemetry";

const POLICY: &str = "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nparallel = 4\ntrust = [\"open\", \"sandbox\", \"container\"]\n\n\
[preview]\ncommand = [\"./preview\"]\n";

struct Rounds(Cell<usize>);

impl Pause for Rounds {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        true
    }
}

/// Whether this is the process `test` runs in. In the one the harness
/// started, it runs `test` alone in a child of this binary, with none of
/// the host's `OTEL_*` or `SLUICE_OTEL*` and a collector no one answers
/// at, asserts it ran and passed, and says no.
fn alone(test: &str) -> bool {
    if std::env::var("SLUICE_TEST_ALONE").as_deref() == Ok(test) {
        return true;
    }
    let mut c = Command::new(std::env::current_exe().unwrap());
    for (k, _) in std::env::vars() {
        if k.starts_with("OTEL_") || k.starts_with("SLUICE_OTEL") {
            c.env_remove(k);
        }
    }
    let out = support::isolated(&mut c)
        .args(["--exact", test, "--nocapture"])
        .env("SLUICE_TEST_ALONE", test)
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", nobody())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && said.contains(&format!("test {test} ... ok")),
        "{test}, alone:\n{said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    false
}

/// A collector's address that no one answers at.
fn nobody() -> String {
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", free.local_addr().unwrap())
}

/// The settings a command given `--otel` and `choices` chooses on a host
/// with no `[telemetry]`, named `host-a`, in this process's environment.
fn asked(choices: Choices) -> Chosen {
    let choices = Choices {
        otel: Some(true),
        ..choices
    };
    Chosen::choose(choices, None, "host-a", std::env::vars())
}

/// The settings `asked` chooses at `logs`.
fn at(logs: Logs) -> Chosen {
    asked(Choices {
        logs: Some(logs),
        ..Choices::default()
    })
}

/// Telemetry sending spans and, unless `logs` is off, logs to memory.
fn start(logs: Logs) -> (Telemetry, InMemorySpanExporter, InMemoryLogExporter) {
    let spans = InMemorySpanExporter::default();
    let records = InMemoryLogExporter::default();
    let telemetry = Telemetry::start_with(&at(logs), spans.clone(), Some(records.clone())).unwrap();
    (telemetry, spans, records)
}

/// A branch whose `tests:t-a` fails once `tests/t_a.py` is broken.
fn setup() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", POLICY),
            ("lib/a.py", "a"),
            ("tests/t_a.py", "t"),
        ],
    );
    f.world.task("tests:t-a", "tests/");
    f.world.fails_with("tests:t-a", "tests/t_a.py", "broken");
    f
}

/// `sluice <args>` over the fakes, its pull requests from `pulls`.
fn sluice(f: &Fakes, pulls: &dyn PullRequests, args: &[&str]) -> i32 {
    under(f, pulls, args, None)
}

/// `sluice <args>` over the fakes, started under `caller`'s trace context.
fn under(f: &Fakes, pulls: &dyn PullRequests, args: &[&str], caller: Option<Caller>) -> i32 {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let cli = Cli { caller, ..cli };
    run(
        cli,
        f.seams(),
        &support::FakeHost::new(f.world.clone(), 0),
        &(),
        pulls,
        &f.clock,
        &Rounds(Cell::new(0)),
        &mut Vec::new(),
    )
    .unwrap()
}

/// `changes` on the branch's tip, submitted from `worktree`: its exit.
fn submit(f: &Fakes, worktree: &str, changes: &[(&str, Option<&str>)]) -> i32 {
    submit_under(f, worktree, changes, None)
}

/// `changes` on the branch's tip, submitted from `worktree` under
/// `caller`'s trace context: its exit.
fn submit_under(
    f: &Fakes,
    worktree: &str,
    changes: &[(&str, Option<&str>)],
    caller: Option<Caller>,
) -> i32 {
    let head = f.world.work(&f.world.tip(BRANCH), changes);
    f.world.session(worktree, "agent-a", &head);
    under(f, &(), &["submit", BRANCH, "--worktree", worktree], caller)
}

/// The trace and span a caller's context names.
const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT: &str = "00f067aa0ba902b7";

/// A caller's context, `TRACEPARENT` `00-<TRACE>-<PARENT>-01`.
fn caller() -> Caller {
    Caller::read([("TRACEPARENT", format!("00-{TRACE}-{PARENT}-01"))]).unwrap()
}

/// Pull requests whose forge cannot be read: a round says so as a
/// warning, with what reads as a token in it.
struct Unreadable;

impl PullRequests for Unreadable {
    fn branches(&self, _: Seams<'_>) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn intake(&self, _: Seams<'_>, _: &dyn Clock, _: &str) -> anyhow::Result<sluice::Intake> {
        anyhow::bail!("the forge refused ghp_0123456789abcdefghijABCDEFGHIJ012345")
    }

    fn open(
        &self,
        _: Seams<'_>,
        _: &dyn Clock,
        _: &SubmitHead,
        branch: &str,
        _: Option<&str>,
    ) -> anyhow::Result<Result<Opened, SubmitRefusal>> {
        anyhow::bail!("{branch} takes no pull requests")
    }

    fn find(&self, _: Seams<'_>, _: &dyn Clock, _: &str, _: &str) -> anyhow::Result<Option<u64>> {
        Ok(None)
    }

    fn holds(&self, _: Seams<'_>, _: &dyn Clock, branch: &str, _: u64) -> anyhow::Result<bool> {
        anyhow::bail!("{branch} takes no pull requests")
    }

    fn follow(
        &self,
        _: Seams<'_>,
        _: &dyn Clock,
        branch: &str,
        _: u64,
    ) -> anyhow::Result<Followed> {
        anyhow::bail!("{branch} takes no pull requests")
    }
}

fn records(logs: &InMemoryLogExporter) -> Vec<SdkLogRecord> {
    logs.get_emitted_logs()
        .unwrap()
        .into_iter()
        .map(|r| r.record)
        .collect()
}

fn is_output(r: &SdkLogRecord) -> bool {
    r.target().map(|t| t.as_ref()) == Some(OUTPUT)
}

fn body(r: &SdkLogRecord) -> String {
    match r.body() {
        Some(AnyValue::String(s)) => s.as_str().to_string(),
        other => panic!("no body: {other:?}"),
    }
}

fn attr(r: &SdkLogRecord, key: &str) -> Option<String> {
    r.attributes_iter()
        .find(|(k, _)| k.as_str() == key)
        .map(|(_, v)| match v {
            AnyValue::String(s) => s.as_str().to_string(),
            other => format!("{other:?}"),
        })
}

/// The span named `name`.
fn span(spans: &InMemorySpanExporter, name: &str) -> SpanData {
    spans
        .get_finished_spans()
        .unwrap()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no {name} span"))
}

/// The value `span` last recorded for `key`.
fn value(span: &SpanData, key: &str) -> Option<Value> {
    span.attributes
        .iter()
        .rev()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.clone())
}

#[test]
fn a_failure_s_output_is_sent_under_its_verdict_scrubbed() {
    if !alone("a_failure_s_output_is_sent_under_its_verdict_scrubbed") {
        return;
    }
    // What the fake check prints begins so: as a secret's value, it is
    // never sent.
    std::env::set_var("SLUICE_TEST_TOKEN", "tests:t-a failed");
    let (telemetry, spans, logs) = start(Logs::Tail);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.flush();

    let records = records(&logs);
    // Only a failure's output and warnings: never a metric's event.
    for r in &records {
        assert!(
            is_output(r) || r.severity_number().is_some_and(|s| s >= Severity::Warn),
            "{r:?}"
        );
    }
    let outputs: Vec<_> = records.iter().filter(|r| is_output(r)).collect();
    assert_eq!(outputs.len(), 1, "{records:?}");
    let out = outputs[0];
    assert!(body(out).starts_with("[redacted] in "), "{out:?}");
    assert_eq!(attr(out, "sluice.task").as_deref(), Some("tests:t-a"));
    assert_eq!(attr(out, "sluice.branch").as_deref(), Some(BRANCH));
    for key in [
        "sluice.candidate",
        "sluice.submissions",
        "sluice.bytes",
        "sluice.replayed",
    ] {
        assert!(attr(out, key).is_some(), "no {key}: {out:?}");
    }
    // The copy kept is the output as it was.
    let kept = attr(out, "sluice.kept").expect("where the output is kept");
    let text = f.world.kept_log(&kept).expect("the kept output");
    assert!(text.starts_with("tests:t-a failed in "), "{text}");

    let verdict = span(&spans, "sluice.verdict");
    let ids = out.trace_context().expect("the record's trace context");
    assert_eq!(ids.trace_id, verdict.span_context.trace_id());
    assert_eq!(ids.span_id, verdict.span_context.span_id());
    // Sent as the service sluice, at this release.
    let resource = &logs.get_emitted_logs().unwrap()[0].resource;
    for (key, want) in [
        ("service.name", "sluice"),
        ("service.version", sluice::VERSION),
    ] {
        let got = resource.get(&opentelemetry::Key::from_static_str(key));
        assert_eq!(got.map(|v| v.to_string()).as_deref(), Some(want), "{key}");
    }
    telemetry.finish();
}

/// At `summary` a failure's record names its task and where its output is
/// kept, never the output; a warning is sent whole, scrubbed.
#[test]
fn a_summary_says_where_the_output_is_kept_and_a_warning_is_sent_whole() {
    if !alone("a_summary_says_where_the_output_is_kept_and_a_warning_is_sent_whole") {
        return;
    }
    let (telemetry, _, logs) = start(Logs::Summary);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    assert_eq!(sluice(&f, &Unreadable, &["step", BRANCH]), LANDED);
    telemetry.flush();

    let records = records(&logs);
    let outputs: Vec<_> = records.iter().filter(|r| is_output(r)).collect();
    assert_eq!(outputs.len(), 1, "{records:?}");
    let kept = attr(outputs[0], "sluice.kept").expect("where the output is kept");
    assert_eq!(
        body(outputs[0]),
        format!("tests:t-a failed; its output is kept at {kept}")
    );
    let warnings: Vec<_> = records
        .iter()
        .filter(|r| r.severity_number() == Some(Severity::Warn))
        .collect();
    assert_eq!(warnings.len(), 1, "{records:?}");
    // Design D10, D26: a step says each cause its round's intake met, as
    // what it asked and the words it was refused with.
    assert_eq!(
        body(warnings[0]),
        format!(
            "sluice: warning: taking in {BRANCH}'s pull requests: {BRANCH}'s pull requests: \
             the forge refused [redacted]"
        )
    );
    telemetry.finish();
}

/// A preview whose command failed sends its output once, when it is
/// released.
#[test]
fn a_failed_preview_s_output_is_sent_when_it_is_released() {
    if !alone("a_failed_preview_s_output_is_sent_when_it_is_released") {
        return;
    }
    let (telemetry, _, logs) = start(Logs::Summary);
    let f = setup();
    f.world.preview_fails(true);
    let rev = f.world.tip(BRANCH);
    let code = sluice(&f, &(), &["preview", BRANCH, rev.as_str(), "--wait"]);
    assert_eq!(code, FAILED);
    let name = format!("preview/{}", &rev.as_str()[..12]);
    assert_eq!(f.world.preview_releases(), vec![(name.clone(), true)]);
    telemetry.flush();

    let records = records(&logs);
    let outputs: Vec<_> = records.iter().filter(|r| is_output(r)).collect();
    assert_eq!(outputs.len(), 1, "{records:?}");
    assert_eq!(attr(outputs[0], "sluice.preview"), Some(name.clone()));
    assert_eq!(attr(outputs[0], "sluice.branch").as_deref(), Some(BRANCH));
    assert_eq!(body(outputs[0]), format!("preview {name} failed"));
    telemetry.finish();
}

/// At `off` no record is sent at all: neither a failure's output nor a
/// warning.
#[test]
fn at_off_no_log_record_is_sent() {
    if !alone("at_off_no_log_record_is_sent") {
        return;
    }
    let (telemetry, spans, logs) = start(Logs::Off);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    assert_eq!(sluice(&f, &Unreadable, &["step", BRANCH]), LANDED);
    telemetry.flush();
    assert!(records(&logs).is_empty());
    span(&spans, "sluice.verdict");
    telemetry.finish();
}

/// A round's spans carry what they are of: the branch, the submissions
/// and the candidate, the executor and whether it confines, and a
/// verdict's outcome and how many tasks ran and replayed.
#[test]
fn a_round_s_spans_carry_what_they_are_of() {
    if !alone("a_round_s_spans_carry_what_they_are_of") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.flush();

    let round = span(&spans, "sluice.round");
    assert_eq!(value(&round, "sluice.branch"), Some(BRANCH.into()));
    let start = span(&spans, "sluice.start");
    let verdict = span(&spans, "sluice.verdict");
    for key in ["sluice.branch", "sluice.submissions", "sluice.candidate"] {
        assert!(value(&start, key).is_some(), "no {key} on sluice.start");
        assert!(value(&verdict, key).is_some(), "no {key} on sluice.verdict");
    }
    assert!(value(&start, "sluice.executor").is_some());
    assert_eq!(value(&verdict, "sluice.executor"), Some("open".into()));
    assert_eq!(value(&verdict, "sluice.confined"), Some(false.into()));
    assert_eq!(value(&verdict, "sluice.outcome"), Some("failed".into()));
    let count = |key| value(&verdict, key).map(|v| v.to_string());
    assert_eq!(count("sluice.ran").as_deref(), Some("1"));
    assert_eq!(count("sluice.replayed").as_deref(), Some("0"));
    let blame = span(&spans, "sluice.blame");
    assert!(value(&blame, "sluice.candidate").is_some());
    telemetry.finish();
}

/// Each round is one trace of its own, rooted at `sluice.round`: its
/// recover, decide and perform beneath it, and what it forms, starts,
/// reads, blames and lands beneath those.
#[test]
fn a_round_is_one_trace_rooted_at_sluice_round() {
    if !alone("a_round_is_one_trace_rooted_at_sluice_round") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    assert_eq!(
        submit(&f, "unit/b", &[("lib/a.py", Some("changed"))]),
        LANDED
    );
    telemetry.flush();

    let all = spans.get_finished_spans().unwrap();
    let by_id: HashMap<SpanId, &SpanData> =
        all.iter().map(|s| (s.span_context.span_id(), s)).collect();
    // The span `s` is beneath, in its own trace.
    let parent = |s: &SpanData| {
        let p = by_id
            .get(&s.parent_span_id)
            .unwrap_or_else(|| panic!("{} has no parent: {s:?}", s.name));
        assert_eq!(
            p.span_context.trace_id(),
            s.span_context.trace_id(),
            "{} is not in {}'s trace",
            s.name,
            p.name
        );
        p.name.to_string()
    };
    let named = |name: &str| -> Vec<&SpanData> { all.iter().filter(|s| s.name == name).collect() };

    let rounds = named("sluice.round");
    for round in &rounds {
        assert_eq!(round.parent_span_id, SpanId::INVALID, "{round:?}");
    }
    let traces: HashSet<TraceId> = rounds.iter().map(|r| r.span_context.trace_id()).collect();
    assert_eq!(traces.len(), rounds.len(), "two rounds share a trace");

    let phases = ["sluice.recover", "sluice.decide", "sluice.perform"];
    for phase in phases {
        let found = named(phase);
        assert!(!found.is_empty(), "no {phase} span");
        for s in found {
            assert_eq!(parent(s), "sluice.round");
        }
    }
    for step in [
        "sluice.form",
        "sluice.start",
        "sluice.verdict",
        "sluice.blame",
        "sluice.land",
    ] {
        let found = named(step);
        assert!(!found.is_empty(), "no {step} span");
        for s in found {
            let p = parent(s);
            assert!(phases.contains(&p.as_str()), "{step} is beneath {p}");
        }
    }
    telemetry.finish();
}

/// 153 (design D2, D3): a submit started under a caller's context sends
/// one `sluice submit` span in the caller's trace, beneath the caller's
/// span and holding its exit; one started under none sends none. Each
/// round it waits on is a trace of its own, nested under no command.
#[test]
fn a_command_nests_under_its_caller_s_trace() {
    if !alone("a_command_nests_under_its_caller_s_trace") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = setup();
    assert_eq!(
        submit_under(
            &f,
            "unit/a",
            &[("tests/t_a.py", Some("broken"))],
            Some(caller())
        ),
        FAILED
    );
    assert_eq!(
        submit(&f, "unit/b", &[("lib/a.py", Some("changed"))]),
        LANDED
    );
    telemetry.flush();

    let all = spans.get_finished_spans().unwrap();
    let named: Vec<&SpanData> = all.iter().filter(|s| s.name == "sluice submit").collect();
    assert_eq!(named.len(), 1, "{all:?}");
    let command = named[0];
    assert_eq!(command.span_context.trace_id().to_string(), TRACE);
    assert_eq!(command.parent_span_id.to_string(), PARENT);
    assert_eq!(
        value(command, "sluice.exit"),
        Some(Value::I64(FAILED.into()))
    );
    assert!(!all.iter().any(|s| s.name == "sluice.command"), "{all:?}");

    let rounds: Vec<&SpanData> = all.iter().filter(|s| s.name == "sluice.round").collect();
    assert!(rounds.len() >= 2, "{rounds:?}");
    let mut traces = HashSet::new();
    for round in &rounds {
        assert_eq!(round.parent_span_id, SpanId::INVALID, "{round:?}");
        let trace = round.span_context.trace_id();
        assert_ne!(trace.to_string(), TRACE, "{round:?}");
        assert!(traces.insert(trace), "two rounds share a trace");
    }
    telemetry.finish();
}

/// 153 (design D5; rule core.53): each `sluice.start` and `sluice.verdict`
/// span of a combination links to the context each of its submissions
/// kept, naming the submission; a submission that kept none gets no link.
#[test]
fn a_round_links_to_the_trace_its_work_was_submitted_under() {
    if !alone("a_round_links_to_the_trace_its_work_was_submitted_under") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = setup();
    let tip = f.world.tip(BRANCH);
    let one = f.world.work(&tip, &[("tests/t_a.py", Some("one"))]);
    f.world.session("unit/a", "agent-a", &one);
    let args = ["submit", BRANCH, "--worktree", "unit/a", "--no-wait"];
    assert_eq!(under(&f, &(), &args, Some(caller())), LANDED);
    let two = f.world.work(&tip, &[("tests/t_b.py", Some("two"))]);
    f.world.session("unit/b", "agent-b", &two);
    let args = ["submit", BRANCH, "--worktree", "unit/b"];
    assert_eq!(under(&f, &(), &args, None), LANDED);
    telemetry.flush();

    let all = spans.get_finished_spans().unwrap();
    let mut seen = 0;
    for s in all
        .iter()
        .filter(|s| s.name == "sluice.start" || s.name == "sluice.verdict")
    {
        let holds = |id: u64| {
            value(s, "sluice.submissions").is_some_and(|v| v.to_string().contains(&id.to_string()))
        };
        assert!(holds(1), "{s:?}");
        let links: Vec<(String, String, Vec<String>)> = s
            .links
            .iter()
            .map(|l| {
                (
                    l.span_context.trace_id().to_string(),
                    l.span_context.span_id().to_string(),
                    l.attributes
                        .iter()
                        .map(|kv| format!("{}={}", kv.key, kv.value))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            links,
            [(
                TRACE.to_string(),
                PARENT.to_string(),
                vec!["sluice.submission=1".to_string()]
            )],
            "{} of {:?}",
            s.name,
            value(s, "sluice.submissions")
        );
        seen += 1;
    }
    // A combination holding both was started and read.
    let both = all.iter().any(|s| {
        s.name == "sluice.verdict"
            && value(s, "sluice.submissions").is_some_and(|v| v.to_string() == "[1, 2]")
    });
    assert!(both, "{all:?}");
    assert!(seen >= 2, "{all:?}");
    telemetry.finish();
}

/// 154 (design D1, D5 of `each-submission-shows-its-wait-and-its-test`;
/// rule core.53): each submission that lands sends one `sluice.submission`
/// span, from its submission to its end: one made under a caller's context
/// in that trace, beneath the caller's span, and one made under none the
/// root of a trace of its own; each `landed`, its seconds waiting and
/// under test summing to its length.
#[test]
fn a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under() {
    if !alone("a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = setup();
    let tip = f.world.tip(BRANCH);
    let one = f.world.work(&tip, &[("tests/t_a.py", Some("one"))]);
    f.world.session("unit/a", "agent-a", &one);
    let args = ["submit", BRANCH, "--worktree", "unit/a"];
    assert_eq!(under(&f, &(), &args, Some(caller())), LANDED);
    let two = f
        .world
        .work(&f.world.tip(BRANCH), &[("tests/t_b.py", Some("two"))]);
    f.world.session("unit/b", "agent-b", &two);
    let args = ["submit", BRANCH, "--worktree", "unit/b"];
    assert_eq!(under(&f, &(), &args, None), LANDED);
    telemetry.flush();

    let all = spans.get_finished_spans().unwrap();
    let lives: Vec<&SpanData> = all
        .iter()
        .filter(|s| s.name == "sluice.submission")
        .collect();
    assert_eq!(lives.len(), 2, "{all:?}");
    let life = |n: i64| {
        *lives
            .iter()
            .find(|s| value(s, "sluice.submission") == Some(Value::I64(n)))
            .unwrap_or_else(|| panic!("no life of #{n} in {lives:?}"))
    };
    let (first, second) = (life(1), life(2));
    assert_eq!(first.span_context.trace_id().to_string(), TRACE);
    assert_eq!(first.parent_span_id.to_string(), PARENT);
    assert_eq!(second.parent_span_id, SpanId::INVALID, "{second:?}");
    assert_ne!(second.span_context.trace_id().to_string(), TRACE);
    let sluice = sluice_core::Sluice::new(f.seams(), &f.clock);
    for (n, span) in [(1, first), (2, second)] {
        let record = sluice.state_on(BRANCH, n).unwrap().expect("its record");
        assert_eq!(span.start_time, time(&record.submitted), "#{n}");
        assert_eq!(value(span, "sluice.outcome"), Some("landed".into()));
        let seconds = |key: &str| match value(span, key) {
            Some(Value::F64(v)) => v,
            other => panic!("{key} of #{n}: {other:?}"),
        };
        let length = span
            .end_time
            .duration_since(span.start_time)
            .unwrap()
            .as_secs_f64();
        assert!(length > 0.0, "#{n}");
        let sum = seconds("sluice.waited_seconds") + seconds("sluice.tested_seconds");
        assert!((sum - length).abs() < 1e-9, "#{n}: {sum} of {length}");
    }
    telemetry.finish();
}

/// A branch with three checks, `tests:t-a`, `tests:t-b` and `tests:t-c`,
/// each reading its own folder, and a worktree `unit/a` whose head
/// changes each, `tests:t-b`'s so it fails; the runner answers
/// `tests:t-c` from an earlier run and times each.
fn three_checks() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", POLICY),
            ("tests/a/t.py", "a"),
            ("tests/b/t.py", "b"),
            ("tests/c/t.py", "c"),
        ],
    );
    for (task, path) in [
        ("tests:t-a", "tests/a/"),
        ("tests:t-b", "tests/b/"),
        ("tests:t-c", "tests/c/"),
    ] {
        f.world.task(task, path);
    }
    f.world.fails_with("tests:t-b", "tests/b/t.py", "broken");
    f.world.replays("tests:t-c");
    f.world
        .times("tests:t-a", "2026-11-18T12:00:01Z", "2026-11-18T12:00:04Z");
    f.world
        .times("tests:t-b", "2026-11-18T12:00:02Z", "2026-11-18T12:00:06Z");
    f.world
        .times("tests:t-c", "2026-11-18T12:00:00Z", "2026-11-18T12:00:01Z");
    let head = f.world.work(
        &f.world.tip(BRANCH),
        &[
            ("tests/a/t.py", Some("a2")),
            ("tests/b/t.py", Some("broken")),
            ("tests/c/t.py", Some("c2")),
        ],
    );
    f.world.session("unit/a", "agent-a", &head);
    f
}

/// An RFC 3339 time as the system's.
fn time(time: &str) -> std::time::SystemTime {
    chrono::DateTime::parse_from_rfc3339(time).unwrap().into()
}

/// The value of `key` among `attributes`.
fn named(attributes: &[opentelemetry::KeyValue], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

/// 153 (design D6): an exec started under a caller's context holds, under
/// its `sluice exec` span, a `sluice.check` span for each check its run
/// ran, over the times its runner gave and with the report's outcome, and
/// none for a check answered from an earlier run; its span holds the
/// branch and how many ran and were replayed.
#[test]
fn an_exec_s_span_holds_each_check_it_ran() {
    if !alone("an_exec_s_span_holds_each_check_it_ran") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = three_checks();
    let args = ["exec", BRANCH, "--worktree", "unit/a"];
    assert_eq!(under(&f, &(), &args, Some(caller())), FAILED);
    telemetry.flush();

    let exec = span(&spans, "sluice exec");
    assert_eq!(exec.span_context.trace_id().to_string(), TRACE);
    assert_eq!(exec.parent_span_id.to_string(), PARENT);
    assert_eq!(value(&exec, "sluice.branch"), Some(BRANCH.into()));
    let count = |key| value(&exec, key).map(|v| v.to_string());
    assert_eq!(count("sluice.ran").as_deref(), Some("2"));
    assert_eq!(count("sluice.replayed").as_deref(), Some("1"));

    let mut checks: Vec<_> = spans
        .get_finished_spans()
        .unwrap()
        .into_iter()
        .filter(|s| s.name == "sluice.check")
        .map(|s| {
            assert_eq!(s.parent_span_id, exec.span_context.span_id(), "{s:?}");
            assert_eq!(
                s.span_context.trace_id(),
                exec.span_context.trace_id(),
                "{s:?}"
            );
            (
                named(&s.attributes, "sluice.task").unwrap_or_default(),
                named(&s.attributes, "sluice.outcome").unwrap_or_default(),
                s.start_time,
                s.end_time,
            )
        })
        .collect();
    checks.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        checks,
        [
            (
                "tests:t-a".to_string(),
                "pass".to_string(),
                time("2026-11-18T12:00:01Z"),
                time("2026-11-18T12:00:04Z")
            ),
            (
                "tests:t-b".to_string(),
                "fail".to_string(),
                time("2026-11-18T12:00:02Z"),
                time("2026-11-18T12:00:06Z")
            ),
        ]
    );
    telemetry.finish();
}

/// 153 (design D6): a command `sluice exec` runs after `--` is its run's
/// one check, its span named by the command's words with what reads as a
/// secret `[redacted]`, as a log's body is.
#[test]
fn an_exec_s_command_is_one_check_named_scrubbed() {
    if !alone("an_exec_s_command_is_one_check_named_scrubbed") {
        return;
    }
    let (telemetry, spans, _) = start(Logs::Off);
    let f = three_checks();
    let token = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    let command = ["env", &format!("API_TOKEN={token}"), "true"].map(String::from);
    f.world.times(
        &sluice::exec::shell_line(&command),
        "2026-11-18T12:00:01Z",
        "2026-11-18T12:00:02Z",
    );
    let args = [
        &["exec", BRANCH, "--worktree", "unit/a", "--"][..],
        &command.each_ref().map(String::as_str),
    ]
    .concat();
    assert_eq!(under(&f, &(), &args, Some(caller())), LANDED);
    telemetry.flush();

    let checks: Vec<SpanData> = spans
        .get_finished_spans()
        .unwrap()
        .into_iter()
        .filter(|s| s.name == "sluice.check")
        .collect();
    assert_eq!(checks.len(), 1, "{checks:?}");
    let task = named(&checks[0].attributes, "sluice.task").unwrap_or_default();
    assert!(task.contains("[redacted]"), "{task}");
    assert!(!task.contains(token), "{task}");
    telemetry.finish();
}

/// A command run's environment, the run started under a `sluice.start`
/// span at a place named `p 1`, and the revision it ran at.
fn a_run_s_environment() -> (String, String) {
    let repo = support::Repo::new("telemetry-run");
    let checks = Checks::new(
        &repo.dir().join(".git"),
        PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
        Instance {
            bound: 2,
            reserved: 0,
            processors: 8,
            forge: None,
            signing: None,
            peers: Default::default(),
            systems: None,
            caches: Vec::new(),
            expire_after: sluice_core::EXPIRE_AFTER,
            telemetry: None,
            aws: Default::default(),
        },
    )
    .with_running(repo.dir().join(".git/host-running"));
    let tmp = repo.dir().join(".git/sluice/.tmp/p1");
    std::fs::create_dir_all(&tmp).unwrap();
    let revision = repo.rev("HEAD");
    let request = RunRequest {
        place: Place {
            name: "p 1".into(),
            path: repo.dir().to_path_buf(),
            tmp: tmp.clone(),
            env: Default::default(),
        },
        revision: Rev::new(revision.clone()),
        required: vec![],
        since: None,
        beyond: Vec::new(),
        limit: Some(Duration::from_secs(60)),
        limits: Limits::default(),
        tests: Settings::from([
            ("runner".into(), Setting::Text("command".into())),
            (
                "command".into(),
                Setting::Text("env > \"$SLUICE_TMP/env\"".into()),
            ),
        ]),
        command: None,
        replay: true,
        system: None,
    };
    let started = tracing::info_span!("sluice.start");
    let handle = started
        .in_scope(|| support::start(&checks, &request))
        .unwrap();
    drop(started);
    let until = Instant::now() + Duration::from_secs(60);
    let report = loop {
        match checks.progress(&handle).unwrap() {
            Progress::Finished(r) => break r,
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(report.tasks[0].outcome, Outcome::Pass);
    let env = std::fs::read_to_string(tmp.join("env")).unwrap();
    (env, revision)
}

/// The value `env`, as `env` prints it, gives `k`.
fn var(env: &str, k: &str) -> Option<String> {
    env.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .map(str::to_string)
}

/// With no `[telemetry]`, a run's checks get sluice's `OTEL_*`, but for
/// metrics, which they never send (151 as amended 2026-11-18; rule
/// core.66), its resource attributes extended with the host's name, which
/// sluice names itself, the round's trace and span, the place and the
/// revision, each value encoded where the variable's syntax would read
/// it, and the span that started the run as `TRACEPARENT`.
#[test]
fn a_run_s_checks_join_the_round() {
    if !alone("a_run_s_checks_join_the_round") {
        return;
    }
    std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "deployment.environment=test");
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap();
    let (telemetry, spans, _) = start(Logs::Off);
    let (env, revision) = a_run_s_environment();
    telemetry.flush();

    let ids = span(&spans, "sluice.start").span_context;
    let (trace, span) = (ids.trace_id().to_string(), ids.span_id().to_string());
    let var = |k: &str| var(&env, k).unwrap_or_else(|| panic!("no {k} in {env}"));
    assert_eq!(var("OTEL_EXPORTER_OTLP_ENDPOINT"), endpoint);
    assert_eq!(var("OTEL_METRICS_EXPORTER"), "none");
    assert_eq!(var("TRACEPARENT"), format!("00-{trace}-{span}-01"));
    assert_eq!(
        var("OTEL_RESOURCE_ATTRIBUTES"),
        format!(
            "deployment.environment=test,host.name=host-a,sluice.trace_id={trace},\
             sluice.span_id={span},sluice.place=p%201,sluice.revision={revision}"
        )
    );
    telemetry.finish();
}

/// 151: where the host's table names a collector, a run's checks get it,
/// protobuf, each signal sluice does not send as `none`, and its resource
/// attributes as sluice names them, `host.name` and the table's, before
/// the round's and the run's own; and none of the caller's OTLP settings
/// or attributes (record 0106; rule core.53; design D6).
#[test]
fn with_the_tables_collector_a_run_gets_it_and_none_of_the_callers() {
    if !alone("with_the_tables_collector_a_run_gets_it_and_none_of_the_callers") {
        return;
    }
    std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", "x-session=s3cr3t-session");
    std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "deployment.environment=session");
    let collector = nobody();
    let table = sending(
        &collector,
        HostTelemetry {
            metrics: Some(false),
            attributes: [("deployment.environment".into(), "prod".into())].into(),
            ..HostTelemetry::default()
        },
    );
    let settings = Chosen::choose(Choices::default(), Some(&table), "host-a", std::env::vars());
    let spans = InMemorySpanExporter::default();
    let telemetry = Telemetry::start_with(
        &settings,
        spans.clone(),
        Some(InMemoryLogExporter::default()),
    )
    .unwrap();
    let (env, revision) = a_run_s_environment();
    telemetry.flush();

    let ids = span(&spans, "sluice.start").span_context;
    let (trace, span) = (ids.trace_id().to_string(), ids.span_id().to_string());
    let var = |k: &str| var(&env, k).unwrap_or_else(|| panic!("no {k} in {env}"));
    assert_eq!(var("OTEL_EXPORTER_OTLP_ENDPOINT"), collector);
    assert_ne!(
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").as_deref(),
        Ok(collector.as_str())
    );
    assert_eq!(var("OTEL_EXPORTER_OTLP_PROTOCOL"), "http/protobuf");
    assert_eq!(var("OTEL_METRICS_EXPORTER"), "none");
    assert_eq!(var("TRACEPARENT"), format!("00-{trace}-{span}-01"));
    assert_eq!(
        var("OTEL_RESOURCE_ATTRIBUTES"),
        format!(
            "host.name=host-a,deployment.environment=prod,sluice.trace_id={trace},\
             sluice.span_id={span},sluice.place=p%201,sluice.revision={revision}"
        )
    );
    for line in env.lines() {
        assert!(!line.starts_with("OTEL_EXPORTER_OTLP_HEADERS="), "{line}");
        assert!(!line.contains("session"), "{line}");
        assert!(
            !line.starts_with("OTEL_TRACES_EXPORTER=") && !line.starts_with("OTEL_LOGS_EXPORTER="),
            "{line}"
        );
    }
    telemetry.finish();
}

/// With traces off a run's checks join no trace: they get sluice's
/// `OTEL_*`, the setting among them, metrics off as ever (rule core.66),
/// and the host's name, the place and the revision as resource
/// attributes, with no `TRACEPARENT` and no trace or span id.
#[test]
fn with_traces_off_a_run_s_checks_join_no_trace() {
    if !alone("with_traces_off_a_run_s_checks_join_no_trace") {
        return;
    }
    std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "deployment.environment=test");
    std::env::set_var("OTEL_TRACES_EXPORTER", "none");
    let (telemetry, spans, _) = start(Logs::Off);
    let (env, revision) = a_run_s_environment();
    telemetry.flush();

    let made = spans.get_finished_spans().unwrap();
    assert!(made.is_empty(), "{made:?}");
    assert_eq!(var(&env, "OTEL_TRACES_EXPORTER").as_deref(), Some("none"));
    assert_eq!(var(&env, "OTEL_METRICS_EXPORTER").as_deref(), Some("none"));
    assert_eq!(var(&env, "TRACEPARENT"), None, "{env}");
    assert_eq!(
        var(&env, "OTEL_RESOURCE_ATTRIBUTES"),
        Some(format!(
            "deployment.environment=test,host.name=host-a,sluice.place=p%201,\
             sluice.revision={revision}"
        ))
    );
    telemetry.finish();
}

/// What sluice exports goes as OTLP over HTTP, protobuf, to
/// `OTEL_EXPORTER_OTLP_ENDPOINT`, as the service it is named and at its
/// release.
#[test]
fn telemetry_goes_to_the_endpoint_as_otlp_over_http() {
    if !alone("telemetry_goes_to_the_endpoint_as_otlp_over_http") {
        return;
    }
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", collector.url());
    let telemetry = Telemetry::start(&asked(Choices {
        service: Some("sluice-under-test".into()),
        ..Choices::default()
    }))
    .unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();

    let seen = collector.requests();
    for path in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        let r = seen
            .iter()
            .find(|r| r.path == path)
            .unwrap_or_else(|| panic!("nothing to {path}: {seen:?}"));
        assert_eq!(r.method, "POST");
        assert_eq!(r.header("content-type"), Some("application/x-protobuf"));
    }
    let traces = seen.iter().find(|r| r.path == "/v1/traces").unwrap();
    for text in ["sluice-under-test", sluice::VERSION, "sluice.verdict"] {
        assert!(traces.body.contains(text), "no {text} in the traces sent");
    }
}

/// The paths a collector is sent to, by telemetry started as `main`
/// starts it at `logs`, while a submission's combination fails its gate.
fn sent(logs: Logs) -> Vec<String> {
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", collector.url());
    let telemetry = Telemetry::start(&at(logs)).unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();
    collector.requests().into_iter().map(|r| r.path).collect()
}

/// A host's `[telemetry]` sending to `collector`, as `table` sets the rest.
fn sending(collector: &str, table: HostTelemetry) -> HostTelemetry {
    HostTelemetry {
        send: Some(true),
        collector: Some(collector.to_string()),
        ..table
    }
}

/// 151: the table's collector takes every signal sluice sends, protobuf
/// at `/v1/<signal>`, and none of the caller's OTLP settings reach it: a
/// session's collector is sent nothing, and its headers go with nothing
/// (record 0106; rule core.53; design D4).
#[test]
fn the_tables_collector_takes_sluices_telemetry_and_none_of_the_callers() {
    if !alone("the_tables_collector_takes_sluices_telemetry_and_none_of_the_callers") {
        return;
    }
    let ok = |_: &support::http::Request| (200, Vec::new(), String::new());
    let table = support::http::Fake::start(ok);
    let session = support::http::Fake::start(ok);
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", session.url());
    std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", "x-session=s3cr3t-session");
    std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
    let settings = Chosen::choose(
        Choices::default(),
        Some(&sending(table.url(), HostTelemetry::default())),
        "host-a",
        std::env::vars(),
    );
    assert!(settings.send);
    let telemetry = Telemetry::start(&settings).unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();

    let seen = table.requests();
    for path in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        let r = seen
            .iter()
            .find(|r| r.path == path)
            .unwrap_or_else(|| panic!("nothing to {path}: {seen:?}"));
        assert_eq!(r.method, "POST");
        assert_eq!(r.header("content-type"), Some("application/x-protobuf"));
    }
    for r in &seen {
        assert_eq!(r.header("x-session"), None, "{r:?}");
    }
    let elsewhere = session.requests();
    assert!(elsewhere.is_empty(), "{elsewhere:?}");
}

/// 151: a table names no collector only where it does not send by
/// itself, since one whose `send` is `true` and names none is refused;
/// a command's own `--otel` beside it sends where
/// `OTEL_EXPORTER_OTLP_ENDPOINT` says, as a host with no table does
/// (record 0106 as amended; design D4).
#[test]
fn with_no_collector_in_the_table_the_environment_names_it() {
    if !alone("with_no_collector_in_the_table_the_environment_names_it") {
        return;
    }
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", collector.url());
    let table = HostTelemetry {
        service: Some("sluice-a".into()),
        ..HostTelemetry::default()
    };
    let otel = Choices {
        otel: Some(true),
        ..Choices::default()
    };
    let settings = Chosen::choose(otel, Some(&table), "host-a", std::env::vars());
    assert!(settings.send);
    let telemetry = Telemetry::start(&settings).unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();
    let seen: Vec<String> = collector.requests().into_iter().map(|r| r.path).collect();
    for path in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        assert!(
            seen.iter().any(|p| p == path),
            "nothing to {path}: {seen:?}"
        );
    }
}

/// The paths the collector the host's table names is sent to, its other
/// keys as `table` sets them, while a submission's combination fails its
/// gate.
fn sent_by(table: HostTelemetry) -> Vec<String> {
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    let table = sending(collector.url(), table);
    let settings = Chosen::choose(Choices::default(), Some(&table), "host-a", std::env::vars());
    let telemetry = Telemetry::start(&settings).unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();
    collector.requests().into_iter().map(|r| r.path).collect()
}

/// 151: the table's `traces = false` sends no span, whatever
/// `OTEL_TRACES_EXPORTER` says; the metrics and a failure's record still
/// go (record 0106; design D3).
#[test]
fn the_table_turns_traces_off_whatever_the_environment_says() {
    if !alone("the_table_turns_traces_off_whatever_the_environment_says") {
        return;
    }
    std::env::set_var("OTEL_TRACES_EXPORTER", "otlp");
    let seen = sent_by(HostTelemetry {
        traces: Some(false),
        ..HostTelemetry::default()
    });
    sent_to(&seen, &["/v1/metrics", "/v1/logs"], "/v1/traces");
}

/// That `seen` holds a request to each path of `to` and none to `not`.
fn sent_to(seen: &[String], to: &[&str], not: &str) {
    for path in to {
        assert!(
            seen.iter().any(|p| p == path),
            "nothing to {path}: {seen:?}"
        );
    }
    assert!(!seen.iter().any(|p| p == not), "sent to {not}: {seen:?}");
}

/// `OTEL_TRACES_EXPORTER=none` turns traces alone off: no span is sent,
/// and the metrics and a failure's record still go.
#[test]
fn with_traces_off_metrics_and_logs_still_go_to_the_endpoint() {
    if !alone("with_traces_off_metrics_and_logs_still_go_to_the_endpoint") {
        return;
    }
    std::env::set_var("OTEL_TRACES_EXPORTER", "none");
    let seen = sent(Logs::Summary);
    sent_to(&seen, &["/v1/metrics", "/v1/logs"], "/v1/traces");
}

/// `OTEL_METRICS_EXPORTER=none` turns metrics alone off.
#[test]
fn with_metrics_off_traces_and_logs_still_go_to_the_endpoint() {
    if !alone("with_metrics_off_traces_and_logs_still_go_to_the_endpoint") {
        return;
    }
    std::env::set_var("OTEL_METRICS_EXPORTER", "none");
    let seen = sent(Logs::Summary);
    sent_to(&seen, &["/v1/traces", "/v1/logs"], "/v1/metrics");
}

/// `OTEL_LOGS_EXPORTER=none` turns logs alone off, whatever
/// `SLUICE_OTEL_LOGS` asks.
#[test]
fn with_logs_off_traces_and_metrics_still_go_to_the_endpoint() {
    if !alone("with_logs_off_traces_and_metrics_still_go_to_the_endpoint") {
        return;
    }
    std::env::set_var("OTEL_LOGS_EXPORTER", "none");
    let seen = sent(Logs::Summary);
    sent_to(&seen, &["/v1/traces", "/v1/metrics"], "/v1/logs");
}

/// `OTEL_SDK_DISABLED=true` leaves telemetry off as if it were never asked
/// for: nothing is sent, a run gets nothing of it, and a failure fails as
/// it does with telemetry off.
#[test]
fn with_the_sdk_disabled_nothing_is_sent() {
    if !alone("with_the_sdk_disabled_nothing_is_sent") {
        return;
    }
    std::env::set_var("OTEL_SDK_DISABLED", "true");
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", collector.url());
    let telemetry = Telemetry::start(&at(Logs::Tail)).unwrap();
    assert!(!sluice::telemetry::on());
    assert!(sluice::telemetry::run_env(&[("sluice.place", "p")]).is_empty());
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.finish();
    let seen = collector.requests();
    assert!(seen.is_empty(), "{seen:?}");
}

/// With traces off no span is made, so a failure's record goes with no
/// trace or span id.
#[test]
fn with_traces_off_a_failure_s_record_names_no_span() {
    if !alone("with_traces_off_a_failure_s_record_names_no_span") {
        return;
    }
    std::env::set_var("OTEL_TRACES_EXPORTER", "none");
    let (telemetry, spans, logs) = start(Logs::Summary);
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("tests/t_a.py", Some("broken"))]),
        FAILED
    );
    telemetry.flush();

    let made = spans.get_finished_spans().unwrap();
    assert!(made.is_empty(), "{made:?}");
    let records = records(&logs);
    let outputs: Vec<_> = records.iter().filter(|r| is_output(r)).collect();
    assert_eq!(outputs.len(), 1, "{records:?}");
    assert_eq!(
        attr(outputs[0], "sluice.task").as_deref(),
        Some("tests:t-a")
    );
    assert!(outputs[0].trace_context().is_none(), "{:?}", outputs[0]);
    telemetry.finish();
}

/// A collector that cannot be reached loses what was held and fails
/// nothing: the submission lands as it would with telemetry off.
#[test]
fn a_collector_out_of_reach_fails_nothing() {
    if !alone("a_collector_out_of_reach_fails_nothing") {
        return;
    }
    let telemetry = Telemetry::start(&at(Logs::Tail)).unwrap();
    let f = setup();
    assert_eq!(
        submit(&f, "unit/a", &[("lib/a.py", Some("changed"))]),
        LANDED
    );
    telemetry.finish();
}
