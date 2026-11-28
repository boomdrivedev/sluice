//! A command line sluice cannot read exits 64, apart from a conflict's 2,
//! and help and the version still exit 0. The command line is read before
//! any repository is opened, so these run nothing else. Two scenarios run
//! the binary for its exits; what a line reads as, refused or not, and
//! what help says, is read in the test's own process, as the binary
//! reads it with `Cli::try_parse`.

mod support;

use std::process::{Command, Output};

use clap::error::ErrorKind;
use clap::Parser as _;
use sluice::Cli;

/// The command line `args`, after `sluice`, as sluice reads it.
fn parsed(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied()))
}

/// What sluice says of `args`, a line it cannot read: clap's error, which
/// the binary prints on stderr and exits USAGE for.
fn refused(args: &[&str]) -> String {
    match parsed(args) {
        Ok(_) => panic!("{args:?} read"),
        Err(e) => {
            assert!(
                !matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion),
                "{args:?}: {e}"
            );
            e.to_string()
        }
    }
}

fn sluice(args: &[&str]) -> Output {
    support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn an_unknown_flag_exits_usage_on_stderr() {
    let out = sluice(&["status", "--stat", "x"]);
    assert_eq!(out.status.code(), Some(sluice::USAGE));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--stat"), "{err}");
}

#[test]
fn an_unknown_command_is_refused() {
    assert!(refused(&["sumbit", "main"]).contains("sumbit"));
}

#[test]
fn a_missing_argument_is_refused() {
    refused(&["submit"]);
}

/// A subject with a forecast or a status is a command line sluice cannot
/// read, saying so.
#[test]
fn a_subject_with_check_or_status_is_refused() {
    for flag in ["--check", "--status"] {
        let err = refused(&["submit", "main", "--subject", "x", flag]);
        assert!(err.contains("--subject"), "{err}");
    }
}

#[test]
fn help_and_version_exit_zero() {
    for args in [&["--help"][..], &["submit", "--help"], &["--version"]] {
        let out = sluice(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(!out.stdout.is_empty(), "{args:?}");
    }
}

/// The help `args` ask for, as the binary prints it.
fn help(args: &[&str]) -> String {
    match parsed(args) {
        Err(e) if e.kind() == ErrorKind::DisplayHelp => e.to_string(),
        Err(e) => panic!("{args:?}: {e}"),
        Ok(_) => panic!("{args:?} asked for no help"),
    }
}

#[test]
fn submit_help_lists_the_exits_and_not_the_telemetry_flags() {
    let text = help(&["submit", "--help"]);
    for exit in [
        "0    landed, whatever failed after the landing (said on stderr)",
        "1    failed",
        "2    a conflict",
        "3    sluice itself failed",
        "4    withdrawn",
        "64   a command line",
        "130  stopped",
    ] {
        assert!(text.contains(exit), "{exit:?} in {text}");
    }
    assert!(!text.contains("--otel"), "{text}");
}

/// ADR 0041: `wait`'s help lists its exits, still waiting among them,
/// and a timeout it cannot read is a usage error.
#[test]
fn wait_help_lists_the_exits_and_a_bad_timeout_is_refused() {
    let text = help(&["wait", "--help"]);
    for exit in [
        "0    landed, whatever failed after the landing (said on stderr)",
        "1    failed",
        "2    a conflict",
        "4    withdrawn",
        "5    still waiting",
        "130  stopped",
        "--timeout 9m --json",
    ] {
        assert!(text.contains(exit), "{exit:?} in {text}");
    }
    assert!(refused(&["wait", "3", "--timeout", "soon"]).contains("--timeout"));
}

/// C.5 branch: accepting a held branch's revision is a `sluice branch`
/// command, with an example.
#[test]
fn branch_accept_help_has_an_example() {
    let text = help(&["branch", "accept", "--help"]);
    assert!(text.contains("<BRANCH> <REVISION>"), "{text}");
    assert!(text.contains("$ sluice branch accept "), "{text}");
    assert!(help(&["branch", "--help"]).contains("accept"));
}

#[test]
fn only_the_root_help_names_the_telemetry_flags() {
    let text = help(&["--help"]);
    for flag in [
        "--otel ",
        "--otel-service-name",
        "--otel-logs ",
        "--otel-logs-limit",
    ] {
        assert!(text.contains(flag), "{flag:?} in {text}");
    }
}

/// Record 0014: the root help names OpenTelemetry's own settings, which
/// turn telemetry, or one signal of it, off.
#[test]
fn the_root_help_names_opentelemetrys_off_switches() {
    let text = help(&["--help"]);
    for setting in [
        "OTEL_SDK_DISABLED=true",
        "OTEL_TRACES_EXPORTER",
        "OTEL_METRICS_EXPORTER",
        "OTEL_LOGS_EXPORTER",
    ] {
        assert!(text.contains(setting), "{setting:?} in {text}");
    }
}

/// 151: the root help names the host's own settings, where telemetry is
/// set up, and the file that holds them (record 0106; design D8).
#[test]
fn the_root_help_names_the_hosts_telemetry_table() {
    let text = help(&["--help"]);
    for words in ["[telemetry]", "~/.config/sluice/config.toml"] {
        assert!(text.contains(words), "{words:?} in {text}");
    }
}

/// A command still takes the telemetry flags the root help names: the
/// line reads, never refused as one with a flag it cannot read (64).
#[test]
fn a_command_still_takes_the_telemetry_flags() {
    parsed(&["submit", "main", "--check", "--otel-logs", "tail"]).expect("read");
}
