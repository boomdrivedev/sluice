//! The `moon` runner while telemetry is on (ADR 0014): moon runs with
//! `--otel`. Over a stand-in for moon that answers what the runner asks
//! before it runs anything, so it needs no moon and is in the fast tier.
//! A test binary of its own, since telemetry is the process's: the
//! runner's tests in `moon.rs` would otherwise all run with it.

#[path = "../support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use opentelemetry_sdk::logs::InMemoryLogExporter;
use opentelemetry_sdk::trace::InMemorySpanExporter;
use sluice::profiles::checks::Context;
use sluice::profiles::moon::Moon;
use sluice::telemetry::{Choices, Logs, Settings as Chosen, Telemetry};
use sluice_seams::{Limits, Place, Rev, RunRequest, Settings};

/// What the runner asks moon before a run: its version, its tasks and
/// its setup.
const MOON: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "moon 2.6.1" ;;
  query) echo '{"tasks":{"a":{"test":{"target":"a:test","tags":["check-fast"],"options":{}}}}}' ;;
esac
"#;

#[test]
fn moon_runs_with_otel_while_telemetry_is_on() {
    // A collector no one answers at.
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let nobody = format!("http://{}", free.local_addr().unwrap());
    drop(free);
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", nobody);
    // This binary runs with the host's environment: a host, or a sluice
    // run, that turns OpenTelemetry's signals off would leave telemetry
    // off here.
    for setting in [
        "OTEL_SDK_DISABLED",
        "OTEL_TRACES_EXPORTER",
        "OTEL_METRICS_EXPORTER",
        "OTEL_LOGS_EXPORTER",
    ] {
        std::env::remove_var(setting);
    }
    // Chosen as a command given `--otel --otel-logs off` chooses on a host
    // with no `[telemetry]`, so the environment says where it goes.
    let settings = Chosen::choose(
        Choices {
            otel: Some(true),
            logs: Some(Logs::Off),
            ..Choices::default()
        },
        None,
        "host-a",
        std::env::vars(),
    );
    let telemetry = Telemetry::start_with(
        &settings,
        InMemorySpanExporter::default(),
        None::<InMemoryLogExporter>,
    )
    .unwrap();

    let scratch = support::Scratch::new("moon-telemetry");
    let bin = scratch.path().join("bin");
    let place = scratch.path().join("place");
    for dir in [&bin, &place] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let moon = bin.join("moon");
    std::fs::write(&moon, MOON).unwrap();
    std::fs::set_permissions(&moon, std::fs::Permissions::from_mode(0o755)).unwrap();
    let request = RunRequest {
        place: Place {
            name: "p1".into(),
            path: place,
            tmp: scratch.path().join("tmp"),
            env: Default::default(),
        },
        revision: Rev::new("0".repeat(40)),
        required: vec!["#check-fast".into()],
        since: None,
        beyond: Vec::new(),
        limit: Some(Duration::from_secs(60)),
        limits: Limits::default(),
        tests: Settings::new(),
        command: None,
        replay: true,
        system: None,
    };
    let env = BTreeMap::from([(
        "PATH".to_string(),
        format!("{}:/usr/bin:/bin", bin.display()),
    )]);
    let cx = Context {
        request: &request,
        env: &env,
        toolchain: None,
        jobs: 2,
    };
    let argv = Moon.plan(&cx).unwrap().argv.expect("a run");
    assert_eq!(argv[..4], ["moon", "--otel", "exec", "a:test"], "{argv:?}");
    telemetry.finish();
}
