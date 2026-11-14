//! What a round reports while telemetry is on (ADRs 0014, 0022): its
//! spans, its metric events and a failure's output, over the fakes; and
//! nothing at all unless telemetry is asked for. What it decides is the same with telemetry on or off, which the
//! CLI's tests show with it off.

mod support;

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use sluice::instance::Instance;
use sluice::profiles::checks::Checks;
use sluice::telemetry::{Choices, Logs};
use sluice::{run, Cli, Pause, CONFLICT, FAILED, LANDED};
use sluice_core::OUTPUT;
use sluice_fakes::{Crash, Fakes};
use sluice_seams::{Limits, Outcome, Place, Progress, Rev, RunRequest, Setting, Settings, Tests};
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

const BRANCH: &str = "bolt/telemetry";

const POLICY: &str = "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nparallel = 4\ntrust = [\"open\", \"sandbox\", \"container\"]\n";

struct Rounds(Cell<usize>);

impl Pause for Rounds {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        true
    }
}

/// Every span's name, every metric event as `<metric> <value>` with its
/// other fields after it as `<field>=<value>`, and every output event as
/// `output <task> in <span>`, in order.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

/// A metric event's metric and value, and its other fields.
#[derive(Default)]
struct Metric {
    metric: Option<String>,
    fields: Vec<String>,
}

impl Metric {
    fn field(&mut self, field: &Field, value: String) {
        let name = field.name();
        if ["monotonic_counter.", "histogram.", "gauge."]
            .iter()
            .any(|p| name.starts_with(p))
        {
            self.metric = Some(format!("{name} {value}"));
        } else {
            self.fields.push(format!("{name}={value}"));
        }
    }
}

impl Visit for Metric {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.field(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.field(field, format!("{value:?}"));
    }
}

/// An output event's task.
struct Task(String);

impl Visit for Task {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "sluice.task" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Seen {
    fn on_new_span(&self, attrs: &Attributes<'_>, _: &Id, _: Context<'_, S>) {
        self.0
            .lock()
            .unwrap()
            .push(attrs.metadata().name().to_string());
    }

    fn on_event(&self, event: &Event<'_>, cx: Context<'_, S>) {
        if event.metadata().target() == OUTPUT {
            let mut task = Task(String::new());
            event.record(&mut task);
            let span = cx.event_span(event).map(|s| s.name()).unwrap_or("none");
            self.0
                .lock()
                .unwrap()
                .push(format!("output {} in {span}", task.0));
        }
        let mut m = Metric::default();
        event.record(&mut m);
        if let Some(metric) = m.metric {
            let line = std::iter::once(metric).chain(m.fields).collect::<Vec<_>>();
            self.0.lock().unwrap().push(line.join(" "));
        }
    }
}

impl Seen {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// `sluice <args>` over the fakes, what it reports kept in `seen`.
fn sluice(f: &Fakes, seen: &Seen, args: &[&str]) -> anyhow::Result<i32> {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let subscriber = tracing_subscriber::registry().with(seen.clone());
    tracing::subscriber::with_default(subscriber, || {
        run(
            cli,
            f.seams(),
            &support::FakeHost::new(f.world.clone(), 0),
            &(),
            &(),
            &f.clock,
            &Rounds(Cell::new(0)),
            &mut Vec::new(),
        )
    })
}

/// `changes` on the branch's tip, made `worktree`'s head.
fn work(f: &Fakes, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) {
    let head = f.world.work(&f.world.tip(BRANCH), changes);
    f.world.session(worktree, agent, &head);
}

fn submit(f: &Fakes, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) -> Vec<String> {
    work(f, worktree, agent, changes);
    let seen = Seen::default();
    let code = sluice(f, &seen, &["submit", BRANCH, "--worktree", worktree]).unwrap();
    let mut seen = seen.lines();
    seen.push(format!("exit {code}"));
    seen
}

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
    f.world.task("lib:test", "lib/");
    f
}

/// Whether `seen` holds `name`: a span, a line, or a metric of any value.
fn has(seen: &[String], name: &str) -> bool {
    seen.iter().any(|s| s == name) || !metric(seen, name).is_empty()
}

/// Each event of metric `name` in `seen`, as `<value> <field>=<value>...`.
fn metric<'a>(seen: &'a [String], name: &str) -> Vec<&'a str> {
    seen.iter()
        .filter_map(|s| s.strip_prefix(name)?.strip_prefix(' '))
        .collect()
}

#[test]
fn a_landing_reports_its_round_verdict_and_landing() {
    let f = setup();
    let seen = submit(&f, "unit/a", "agent-a", &[("lib/a.py", Some("changed"))]);
    assert!(has(&seen, &format!("exit {LANDED}")), "{seen:?}");
    for name in [
        "sluice.round",
        "sluice.recover",
        "sluice.decide",
        "sluice.perform",
        "sluice.form",
        "sluice.start",
        "sluice.verdict",
        "sluice.land",
        "gauge.sluice.queue.depth",
        "gauge.sluice.window",
        "monotonic_counter.sluice.verdicts",
        "monotonic_counter.sluice.tasks.ran",
        "monotonic_counter.sluice.tasks.replayed",
        "monotonic_counter.sluice.landed",
        "histogram.sluice.submission.landed_after",
        "histogram.sluice.candidate.tested_for",
    ] {
        assert!(has(&seen, name), "no {name} in {seen:?}");
    }
    assert!(!has(&seen, "sluice.blame"), "{seen:?}");
    assert!(!seen.iter().any(|s| s.starts_with("output ")), "{seen:?}");
    // Each metric is the branch's, and a verdict is counted by its outcome.
    let branch = format!("sluice.branch={BRANCH}");
    let metrics = ["monotonic_counter.", "histogram.", "gauge."];
    for line in seen
        .iter()
        .filter(|s| metrics.iter().any(|m| s.starts_with(m)))
    {
        assert!(line.split(' ').any(|f| f == branch), "{line}");
    }
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.verdicts"),
        [format!("1 {branch} sluice.outcome=passed")]
    );
}

#[test]
fn a_failure_reports_its_culprit() {
    let f = setup();
    f.world.fails_with("tests:t-a", "tests/t_a.py", "broken");
    let seen = submit(&f, "unit/b", "agent-b", &[("tests/t_a.py", Some("broken"))]);
    assert!(has(&seen, &format!("exit {FAILED}")), "{seen:?}");
    for name in ["sluice.blame", "monotonic_counter.sluice.culprits"] {
        assert!(has(&seen, name), "no {name} in {seen:?}");
    }
    assert!(!has(&seen, "sluice.land"), "{seen:?}");
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.verdicts"),
        [format!("1 sluice.branch={BRANCH} sluice.outcome=failed")]
    );
    // ADR 0022, 20: the failed task's output, once, in the verdict's span.
    let outputs: Vec<&String> = seen.iter().filter(|s| s.starts_with("output ")).collect();
    assert_eq!(outputs, ["output tests:t-a in sluice.verdict"], "{seen:?}");
}

/// A task whose key passed before is replayed, and counted as replayed,
/// not ran: `lib/` back as a landing before had it.
#[test]
fn a_replay_is_counted() {
    let f = setup();
    for (unit, agent, text) in [("unit/a", "agent-a", "one"), ("unit/b", "agent-b", "two")] {
        let seen = submit(&f, unit, agent, &[("lib/a.py", Some(text))]);
        assert!(has(&seen, &format!("exit {LANDED}")), "{seen:?}");
    }
    let seen = submit(&f, "unit/c", "agent-c", &[("lib/a.py", Some("one"))]);
    assert!(has(&seen, &format!("exit {LANDED}")), "{seen:?}");
    let branch = format!("sluice.branch={BRANCH}");
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.tasks.replayed"),
        [format!("1 {branch}")],
        "{seen:?}"
    );
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.tasks.ran"),
        [format!("0 {branch}")],
        "{seen:?}"
    );
}

/// A submission whose work conflicts with the branch is counted.
#[test]
fn a_conflict_is_counted() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("lib/a.py", Some("mine"))]);
    f.world
        .hand_commit(BRANCH, &[("lib/a.py", Some("by hand"))]);
    let seen = Seen::default();
    let code = sluice(&f, &seen, &["submit", BRANCH, "--worktree", "unit/a"]).unwrap();
    let seen = seen.lines();
    assert_eq!(code, CONFLICT, "{seen:?}");
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.conflicts"),
        [format!("1 sluice.branch={BRANCH}")],
        "{seen:?}"
    );
}

/// A landing the branch moved under is refused, and counted.
#[test]
fn a_refused_landing_is_counted() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("lib/a.py", Some("changed"))]);
    let seen = Seen::default();
    let step = |seen: &Seen| sluice(&f, seen, &["step", BRANCH]);
    sluice(
        &f,
        &seen,
        &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"],
    )
    .unwrap();
    step(&seen).unwrap();
    // The verdict is read and the landing written; the step stops before
    // the move, and someone moves the branch by hand.
    f.world.crash(Crash::BeforeMove);
    assert!(step(&seen).is_err());
    f.world.hand_commit(BRANCH, &[("c/lib", Some("by hand"))]);
    step(&seen).unwrap();
    let seen = seen.lines();
    assert_eq!(
        metric(&seen, "monotonic_counter.sluice.refused"),
        [format!("1 sluice.branch={BRANCH}")],
        "{seen:?}"
    );
    assert!(!has(&seen, "monotonic_counter.sluice.landed"), "{seen:?}");
}

/// A command chooses only what its flags or its `SLUICE_OTEL*` variables
/// give it, each `None` where it was given neither, so the host's
/// `[telemetry]` holds (151; record 0106; design D2); and while telemetry
/// is off a run's checks get none of sluice's `OTEL_*`, no `TRACEPARENT`
/// and no `sluice.*` attribute.
#[test]
fn telemetry_is_off_unless_asked() {
    let parse = |args: &[&str]| {
        Cli::try_parse_from(["sluice", "step", BRANCH].iter().chain(args).copied())
            .unwrap()
            .choices()
    };
    let variables = [
        "SLUICE_OTEL",
        "SLUICE_OTEL_SERVICE_NAME",
        "SLUICE_OTEL_LOGS",
        "SLUICE_OTEL_LOGS_LIMIT",
    ];
    for k in variables {
        std::env::remove_var(k);
    }
    assert_eq!(parse(&[]), Choices::default());
    assert_eq!(
        parse(&[
            "--otel",
            "--otel-service-name",
            "ci-sluice",
            "--otel-logs",
            "tail",
            "--otel-logs-limit",
            "100",
        ]),
        Choices {
            otel: Some(true),
            service: Some("ci-sluice".into()),
            logs: Some(Logs::Tail),
            logs_limit: Some(100),
        }
    );
    assert_eq!(parse(&["--otel=false"]).otel, Some(false));
    assert_eq!(parse(&["--otel-logs", "off"]).logs, Some(Logs::Off));
    for (value, on) in [
        ("1", true),
        ("true", true),
        ("yes", true),
        ("on", true),
        ("0", false),
        ("off", false),
    ] {
        std::env::set_var("SLUICE_OTEL", value);
        assert_eq!(parse(&[]).otel, Some(on), "SLUICE_OTEL={value}");
    }
    std::env::remove_var("SLUICE_OTEL");
    std::env::set_var("SLUICE_OTEL_SERVICE_NAME", "ci-sluice");
    std::env::set_var("SLUICE_OTEL_LOGS", "tail");
    std::env::set_var("SLUICE_OTEL_LOGS_LIMIT", "100");
    assert_eq!(
        parse(&[]),
        Choices {
            otel: None,
            service: Some("ci-sluice".into()),
            logs: Some(Logs::Tail),
            logs_limit: Some(100),
        }
    );
    for k in variables {
        std::env::remove_var(k);
    }

    // Nothing in this process started telemetry: a run gets nothing of
    // it, though sluice's own environment names a collector.
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4318");
    std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "deployment.environment=test");
    let repo = support::Repo::new("telemetry-off");
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
        },
    )
    .with_running(repo.dir().join(".git/host-running"));
    let tmp = repo.dir().join(".git/sluice/.tmp/p1");
    std::fs::create_dir_all(&tmp).unwrap();
    let request = RunRequest {
        place: Place {
            name: "p1".into(),
            path: repo.dir().to_path_buf(),
            tmp: tmp.clone(),
            env: Default::default(),
        },
        revision: Rev::new(repo.rev("HEAD")),
        required: vec![],
        since: None,
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
    let handle = support::start(&checks, &request).unwrap();
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
    for line in env.lines() {
        assert!(
            !line.starts_with("OTEL_") && !line.starts_with("TRACEPARENT="),
            "{line}"
        );
        assert!(!line.contains("sluice.trace_id"), "{line}");
    }
}
