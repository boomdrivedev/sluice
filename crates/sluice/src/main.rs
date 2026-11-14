use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser;
use sluice::exec::Host;
use sluice::hand_over::HandOver;
use sluice::host_settings::{HostSettings, Look};
use sluice::instance::{Instance, AFRESH};
use sluice::profiles::Bound;
use sluice::retry::Policy;
use sluice::telemetry::{Settings, Telemetry};
use sluice::{drive, run, Carried, Cli, Command, Driven, Driving, Pause, Skipped, Sleep};
use sluice_core::{Clock, Sluice, SystemClock};

fn main() {
    // 135: what a `sluice run` this process started afresh from handed it,
    // read first, before any thread or child starts, and taken out of the
    // environment so no child inherits it (record 0107; design D4).
    let handed = HandOver::take().unwrap_or_else(|e| {
        eprintln!("sluice: {e:#}");
        std::process::exit(3);
    });
    // The run as it started, which a `sluice run` starts afresh as: its
    // environment before telemetry takes anything out of it, its
    // executable through every link, and its command line (151; rule
    // core.53; design D3).
    let started = Started {
        env: std::env::vars_os().collect(),
        program: std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|e| format!("the executable this sluice run started as: {e}")),
        args: std::env::args_os().skip(1).collect(),
    };
    let cli = Cli::try_parse().unwrap_or_else(|e| {
        // Help and the version exit 0; anything sluice cannot read exits
        // USAGE, not clap's 2, which is a conflict's.
        let code = if e.exit_code() == 0 { 0 } else { sluice::USAGE };
        let _ = e.print();
        std::process::exit(code);
    });
    let json = cli.json();
    // A supervisor needs no repository: its run directory says everything.
    if let Some(Command::Supervise { dir }) = &cli.command {
        if let Err(e) = sluice::runs::supervise(dir) {
            eprintln!("sluice supervise: {e:#}");
            std::process::exit(3);
        }
        return;
    }
    // git's credential helper needs no repository: what it gives is in
    // its environment.
    if let Some(Command::Credential { operation }) = &cli.command {
        let mut out = std::io::stdout();
        if let Err(e) = sluice::identity::helper(operation, &mut std::io::stdin().lock(), &mut out)
        {
            eprintln!("sluice credential: {e:#}");
            std::process::exit(3);
        }
        return;
    }
    // 1: whatever runs sluice's work keeps its branches protected; 124:
    // every other command puts back for this sluice a hook installed for
    // another, and installs none. `protect` installs or removes its own,
    // and the hook runs on every ref update.
    match cli.command {
        Some(
            Command::Submit { .. }
            | Command::Wait { .. }
            | Command::Step { .. }
            | Command::Run { .. },
        ) => {
            if let Err(e) = protection(sluice::protect::install) {
                eprintln!("sluice: warning: branch protection not installed: {e:#}");
            }
        }
        Some(Command::Protect { .. } | Command::Hook { .. }) => {}
        _ => {
            if let Err(e) = protection(sluice::protect::reinstall) {
                eprintln!("sluice: warning: branch protection not re-installed: {e:#}");
            }
        }
    }
    let retry = Rc::new(retry(&cli));
    // The forge check reads the host's config itself, and says what in it
    // is wrong rather than failing on it.
    if let Some(Command::Forge {
        command: sluice::ForgeCommand::Check { branch, json },
    }) = &cli.command
    {
        let code = std::env::current_dir()
            .map_err(anyhow::Error::from)
            .and_then(|dir| {
                sluice::profiles::forge_check(
                    &dir,
                    branch.as_deref(),
                    *json,
                    &mut std::io::stdout(),
                    retry.clone(),
                )
            })
            .unwrap_or_else(|e| {
                eprintln!("sluice: {e:#}");
                tell_json(*json, &e);
                3
            });
        std::process::exit(code);
    }
    // Every ref update in every worktree runs the hook: decide nothing
    // unless git asks for the decision. Whatever the hook decides, it
    // reads the whole transaction first: git runs it with SIGPIPE
    // ignored, so a writer that met the pipe closed, the shim's `printf`
    // among them, would say so on the caller's terminal (124).
    if let Some(Command::Hook { state, .. }) = &cli.command {
        let code = if state == "prepared" {
            hook(cli, retry)
        } else {
            0
        };
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        std::process::exit(code);
    }
    // The host's own settings, read once: telemetry starts from them before
    // the bound opens on them and before the interrupt handler is set, so
    // no thread of sluice's runs while telemetry takes the caller's OTLP
    // variables out (151; record 0106; design D2, D4). A `sluice run`
    // holds them with the sign of their file, to take a change up between
    // its passes; one started afresh holds the settings it was handed,
    // under no sign, so its first look reads the file once (135; record
    // 0107; design D4 of `a-kept-run-hands-over-what-it-cannot-take-up`).
    let (read, handed) = match (&cli.command, handed) {
        (Some(Command::Run { .. }), Some((doc, carried))) => (
            HostSettings::read_handed(&doc.settings)
                .map(|settings| (settings.held().clone(), Some(settings))),
            Some(Handed { doc, carried }),
        ),
        (Some(Command::Run { .. }), None) => (
            HostSettings::read().map(|settings| (settings.held().clone(), Some(settings))),
            None,
        ),
        _ => (Instance::read().map(|instance| (instance, None)), None),
    };
    let (instance, held) = match read {
        Ok(read) => read,
        Err(e) => {
            eprintln!("sluice: {e:#}");
            tell_json(json, &e);
            std::process::exit(3);
        }
    };
    let settings = Settings::choose(
        cli.choices(),
        instance.telemetry.as_ref(),
        &sluice::profiles::checks::hostname(),
        std::env::vars(),
    );
    // ADR 0014: telemetry that cannot start is reported and left off.
    let telemetry = if settings.send {
        match Telemetry::start(&settings) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("sluice: warning: telemetry off: {e:#}");
                None
            }
        }
    } else {
        None
    };
    let opened = repository_dir(&cli).and_then(|dir| {
        let program = std::env::current_exe()?;
        let bound = Bound::open_with(&dir, program.clone(), instance, retry.clone())?;
        let kept = held.map(|settings| Kept {
            dir,
            program,
            settings,
            retry: retry.clone(),
            started,
            handed,
        });
        Ok((bound, kept))
    });
    let (bound, kept) = match opened {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("sluice: {e:#}");
            tell_json(json, &e);
            finish(telemetry);
            std::process::exit(3);
        }
    };
    if let Err(e) = sluice::stop_on_interrupt() {
        eprintln!("sluice: {e:#}");
        tell_json(json, &e);
        finish(telemetry);
        std::process::exit(3);
    }
    // The tick a reader names in its document is the pause it takes.
    let every = sluice::tick(cli.command.as_ref());
    let mut out = std::io::stdout();
    let skipped = Skipped::new(every);
    let (clock, pause): (&dyn Clock, &dyn Pause) = match std::env::var(ROUND_SLEEP_VAR).as_deref() {
        Ok("record") => (&skipped, &skipped),
        _ => (&SystemClock, &Sleep(every)),
    };
    let ran = match (kept, &cli.command) {
        (Some(kept), Some(Command::Run { rounds })) => keep_running(
            kept,
            bound,
            *rounds,
            telemetry.as_ref(),
            clock,
            pause,
            &mut out,
        ),
        _ => run(
            cli,
            bound.seams(),
            &bound,
            &bound.branch,
            &*bound.pulls,
            clock,
            pause,
            &mut out,
        ),
    };
    let code = match ran {
        Ok(code) => code,
        Err(e) => {
            sluice::telemetry::error(&format!("sluice: {e:#}"));
            tell_json(json, &e);
            3
        }
    };
    finish(telemetry);
    std::process::exit(code);
}

/// Send what telemetry holds, where it started.
fn finish(telemetry: Option<Telemetry>) {
    if let Some(t) = telemetry {
        t.finish();
    }
}

/// What a `sluice run` keeps to bind its repository afresh: the folder it
/// opened it from, the executable that supervises its check runs, the
/// host's settings it holds with their file's sign, and the retry policy
/// its calls to a remote run under (134, 135); and, to start itself
/// afresh, the run as it started and what it was handed where it started
/// afresh itself (135; record 0107; design D3, D4).
struct Kept {
    dir: PathBuf,
    program: PathBuf,
    settings: HostSettings,
    retry: Rc<Policy>,
    started: Started,
    handed: Option<Handed>,
}

/// The run as it started, which it starts afresh as (design D3): its
/// environment before telemetry took the caller's OTLP variables out of
/// it, the executable it started as resolved through every link, or why
/// that could not be read, and its arguments.
struct Started {
    env: Vec<(OsString, OsString)>,
    program: Result<PathBuf, String>,
    args: Vec<OsString>,
}

/// What the `sluice run` this one started afresh from handed it (design
/// D4): the document, its settings, tables and supervisors, and what its
/// `Driving` carries.
struct Handed {
    doc: HandOver,
    carried: Carried,
}

/// `sluice run`, one pass at a time, taking up the host's settings between
/// two passes once their file changes and never within one (135; record
/// 0107; rule core.54; design D3 to D5). Settings that read are bound
/// afresh, and the new binding's host systems checked (87), before the
/// old binding is let go; settings that do not read or bind leave it on
/// what it holds, said once for that change. A run already going keeps
/// what it was started with: its request and its record hold it.
///
/// A change that moves a table no binding takes up in place, one of
/// [`AFRESH`], is bound and checked the same way and the new binding let
/// go; what `telemetry` holds is sent; and the run starts itself afresh in
/// its own process, as it started, handing over the settings it bound, its
/// driver's lock, its passes, its supervisors and what it said. A start
/// that fails leaves it on everything it holds, said once for that
/// change. A run started afresh says what it took up once it is bound,
/// reaps the supervisors it was handed and drives on from what it was
/// handed (135; record 0107; rule core.54; design D1 to D7 of
/// `a-kept-run-hands-over-what-it-cannot-take-up`).
fn keep_running(
    mut kept: Kept,
    mut bound: Bound,
    rounds: Option<usize>,
    telemetry: Option<&Telemetry>,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> anyhow::Result<i32> {
    let file = kept
        .settings
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let mut driving = match kept.handed.take() {
        Some(Handed { doc, carried }) => {
            sluice::runs::reap_handed(&doc.supervisors);
            eprintln!(
                "sluice: took up the host's settings in {file}: {} changed, in this sluice run \
                 started afresh",
                doc.tables.join(", ")
            );
            Driving::handed(rounds, carried)
        }
        None => Driving::new(rounds),
    };
    loop {
        let driven = drive(
            &mut driving,
            bound.seams(),
            &bound,
            &*bound.pulls,
            clock,
            pause,
            out,
        )?;
        if let Driven::Ended(code) = driven {
            return Ok(code);
        }
        let refused = |why: &str| {
            sluice::telemetry::warn(&format!(
                "sluice: warning: {file} is refused, so this sluice run keeps the settings it \
                 holds until the file changes: {why}"
            ))
        };
        // A change it takes up only by starting afresh that it could not
        // start afresh for, because its settings do not bind or the start
        // failed, said once with the tables (rule core.54; design D5).
        let not_afresh = |tables: &[&str], cause: &str, why: &str| {
            sluice::telemetry::warn(&format!(
                "sluice: warning: {file} changed {}, which this sluice run takes up only by \
                 starting afresh, and {cause}, so it keeps the settings it holds until the file \
                 changes: {why}",
                tables.join(", ")
            ))
        };
        match kept.settings.look() {
            Look::Same => {}
            Look::Refused(why) => refused(&why),
            Look::Changed(instance, text) => {
                let tables = kept.settings.held().changed_tables(&instance);
                let afresh = tables.iter().any(|t| AFRESH.contains(t));
                let rebound = Bound::open_with(
                    &kept.dir,
                    kept.program.clone(),
                    instance.clone(),
                    kept.retry.clone(),
                )
                .and_then(|b| {
                    Sluice::new(b.seams(), clock)
                        .as_peer(b.peer())
                        .check_host_systems()?;
                    Ok(b)
                });
                match rebound {
                    Ok(b) if afresh => {
                        drop(b);
                        if let Some(t) = telemetry {
                            t.flush();
                        }
                        let _ = out.flush();
                        let why = start_afresh(&kept.started, &driving, &tables, text);
                        not_afresh(&tables, "it could not start afresh", &format!("{why:#}"));
                    }
                    Ok(b) => {
                        let tables = tables.join(", ");
                        bound = b;
                        kept.settings.accept(instance);
                        eprintln!(
                            "sluice: took up the host's settings in {file}: {tables} changed"
                        );
                    }
                    Err(e) if afresh => {
                        not_afresh(&tables, "its settings do not bind", &format!("{e:#}"))
                    }
                    Err(e) => refused(&format!("{e:#}")),
                }
            }
        }
    }
}

/// Start this `sluice run` afresh in its own process, as it `started`,
/// handing it `text`, the settings it read and bound, the `tables` that
/// changed, what `driving` carries and the supervisors it has not reaped
/// (135; record 0107; design D2 to D5). It returns only where the start
/// failed, with why; the run then keeps its driver and all it holds.
fn start_afresh(
    started: &Started,
    driving: &Driving,
    tables: &[&str],
    text: String,
) -> anyhow::Error {
    let program = match &started.program {
        Ok(program) => program,
        Err(why) => return anyhow::anyhow!("{why}"),
    };
    let carried = match driving.hand_over() {
        Ok(carried) => carried,
        Err(e) => return e,
    };
    let doc = HandOver {
        settings: text,
        tables: tables.iter().map(|t| t.to_string()).collect(),
        passes: carried.passes,
        driver: None,
        supervisors: sluice::runs::unreaped(),
        said: carried.said,
        passed: carried.passed,
        said_beside: carried.said_beside,
    };
    doc.start(program, &started.args, &started.env, carried.driver)
}

/// The folder the repository is opened from: the one the command runs
/// in, but for `sluice equip`, which equips the worktree holding that
/// folder (147), the worktree's top where git finds one, so it runs from
/// any folder of the worktree. Every other command opens the folder it
/// runs in, so none takes a folder below a worktree's top for the
/// worktree.
fn repository_dir(cli: &Cli) -> anyhow::Result<PathBuf> {
    let dir = std::env::current_dir()?;
    if let Some(Command::Equip { .. }) = &cli.command {
        if let Some(top) = sluice::profiles::worktree_top(&dir)? {
            return Ok(PathBuf::from(top));
        }
    }
    Ok(dir)
}

/// 124: the protection hook, judging the repository git names: `GIT_DIR`
/// when git set it, resolved against the folder the command ran in, and
/// that folder only when git set none. It reads the host's settings and
/// the policy only as far as guarding needs. A transaction it cannot read
/// for a reason of its own passes, said once; it never exits 3.
fn hook(cli: Cli, retry: Rc<Policy>) -> i32 {
    let bound = std::env::current_dir()
        .map(|dir| match std::env::var_os("GIT_DIR") {
            Some(git_dir) if !git_dir.is_empty() => dir.join(git_dir),
            _ => dir,
        })
        .map_err(anyhow::Error::from)
        .and_then(|dir| Bound::guarding(&dir, retry));
    let bound = match bound {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}", sluice::protect::unread(&e));
            return 0;
        }
    };
    let mut out = std::io::stdout();
    match run(
        cli,
        bound.seams(),
        &bound,
        &bound.branch,
        &*bound.pulls,
        &SystemClock,
        &Sleep(Duration::from_secs(3)),
        &mut out,
    ) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", sluice::protect::unread(&e));
            0
        }
    }
}

/// The retry policy every call to a remote runs under (134): a wait's five
/// minutes, ending at its `--timeout`, for a waiting submit and `sluice
/// wait`, and thirty seconds for anything else. A spent allowance is
/// waited out by a waiting submit and `sluice wait` within their
/// `--timeout`, and by any other command but a round of `sluice run` or
/// `sluice step` and a submit that does not wait, which stop on it at
/// once (134 as amended 2026-10-28; the user, 2026-10-28). Tests that run
/// the binary set `SLUICE_RETRY_SLEEP=record` to skip its pauses.
fn retry(cli: &Cli) -> Policy {
    let policy = match &cli.command {
        Some(Command::Wait { timeout, .. }) => Policy::waiting(timeout.map(|t| Instant::now() + t)),
        Some(Command::Submit {
            no_wait: false,
            check: false,
            status: false,
            ..
        }) => Policy::waiting(None),
        Some(Command::Submit { no_wait: true, .. }) => Policy::command().unpaused(),
        Some(Command::Run { .. } | Command::Step { .. }) => Policy::round(),
        _ => Policy::command(),
    };
    match std::env::var(RETRY_SLEEP_VAR).as_deref() {
        Ok("record") => policy.recording(),
        _ => policy,
    }
}

/// Set to `record` in tests, the retry policy's pauses are skipped.
const RETRY_SLEEP_VAR: &str = "SLUICE_RETRY_SLEEP";

/// Set to `record` in tests, the pauses between a run's or a wait's rounds
/// are skipped, and the clock the command reads moved on by each.
const ROUND_SLEEP_VAR: &str = "SLUICE_ROUND_SLEEP";

/// `put` sluice's hook, for this executable, in the repository holding the
/// folder the command runs in.
fn protection(put: fn(&Path, &Path) -> anyhow::Result<bool>) -> anyhow::Result<bool> {
    let dir = std::env::current_dir()?;
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    put(&dir, &exe)
}

/// sluice failing itself (exit 3), told on stdout as well when the reader
/// asked for JSON and waits for it there.
fn tell_json(json: bool, e: &anyhow::Error) {
    if json {
        let d = sluice::told::document(sluice::told::Kind::Internal, &format!("{e:#}"), &[]);
        println!("{d}");
    }
}
