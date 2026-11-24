use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser;
use sluice::hand_over::HandOver;
use sluice::host_settings::{HostSettings, Look};
use sluice::instance::{Instance, AFRESH};
use sluice::profiles::Bound;
use sluice::retry::Policy;
use sluice::telemetry::{Caller, Settings, Telemetry};
use sluice::{run, Cli, Command, Driven, Following, Pause, Skipped, Sleep};
use sluice_core::{Clock, SystemClock};

fn main() {
    // 135: what a `sluice run` this process started afresh from handed it,
    // read first, before any thread or child starts, and taken out of the
    // environment so no child inherits it (record 0107; design D4).
    let handed = HandOver::take().unwrap_or_else(|e| {
        eprintln!("sluice: {e:#}");
        std::process::exit(3);
    });
    // 135: what a `sluice daemon` this process started afresh from in
    // place handed it, its start lock and its socket, or what a host
    // process that hands over to it handed it, each variable taken out of
    // the environment beside the hand-over's, so no child inherits it
    // (design D6 of `sluice-runs-as-a-daemon` and of
    // `sluice-host-runs-on-a-tokio-edge`).
    let carried = sluice::host_process::take_carried();
    raise_open_files();
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
    // 153 (design D1): the trace context the command was started under,
    // read once from the environment it started with, which names only
    // where its span nests.
    let cli = Cli {
        caller: Caller::read(started.env.iter().map(|(k, v)| (k, v))),
        ..cli
    };
    let json = cli.json();
    // A supervisor needs no repository: its run directory says everything.
    if let Some(Command::Supervise { dir }) = &cli.command {
        if let Err(e) = sluice::runs::supervise(dir) {
            eprintln!("sluice supervise: {e:#}");
            std::process::exit(3);
        }
        return;
    }
    // The host process needs no repository: it holds every repository its
    // index names (135; design D1).
    if let Some(Command::Host | Command::Daemon) = &cli.command {
        // A host process keeps no descriptor that its starter's caller
        // left open (`nothing-detached@f807c7f5fcea`).
        sluice::host_process::close_inherited();
    }
    if let Some(Command::Host) = &cli.command {
        match sluice::host_process::run(cli.choices(), carried) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                eprintln!("sluice host: {e:#}");
                std::process::exit(3);
            }
        }
    }
    // `sluice daemon` is the host process kept running, and needs no
    // repository either (137; design D1 of `sluice-runs-as-a-daemon`).
    if let Some(Command::Daemon) = &cli.command {
        match sluice::host_process::daemon(cli.choices(), carried) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                eprintln!("sluice daemon: {e:#}");
                std::process::exit(3);
            }
        }
    }
    drop(carried);
    // `sluice hand-over` needs no repository: it asks the host process
    // (the change `sluice-host-owns-its-workers`, design D5).
    if let Some(Command::HandOver) = &cli.command {
        let asked = sluice::host_process::client::Client::host().and_then(|c| c.hand_over_here());
        match asked {
            Ok(Some((before, after))) => {
                let ran = before.program.map_or_else(
                    || format!("sluice {}", before.version),
                    |p| p.display().to_string(),
                );
                let now = after.program.map_or_else(
                    || format!("sluice {}", after.version),
                    |p| p.display().to_string(),
                );
                println!(
                    "sluice: the host process {}, which ran {ran}, started afresh as {now}",
                    after.pid
                );
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("sluice hand-over: {e:#}");
                std::process::exit(3);
            }
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
    // A read is answered by sluice's host process: the command sends its
    // folder and prints the answer, and binds no repository (the change
    // `sluice-host-answers-every-read`, design D1, D2).
    if let Some(code) = read(&cli) {
        std::process::exit(code);
    }
    // A change is made by sluice's host process: the command sends its
    // folder, its arguments and its caller's session, and prints the
    // answer, and binds no repository (the change
    // `sluice-host-makes-each-change`, design D1, D2).
    if let Some(code) = change(&cli) {
        std::process::exit(code);
    }
    // A watch follows sluice's host process on a subscription, and binds
    // no repository either (design D7).
    if let Some(code) = watch(&cli) {
        std::process::exit(code);
    }
    // A wait follows sluice's host process, which takes each of its steps,
    // a pull request's among them, and binds no repository (design D8 of
    // `sluice-host-answers-every-read`, D2 of
    // `sluice-host-makes-each-change`).
    if let Some(code) = wait(&cli) {
        std::process::exit(code);
    }
    // A session's command runs in sluice's host process: the command sends
    // its folder and prints what the host process sends as it comes, and
    // binds no repository (the change `sluice-host-runs-a-sessions-checks`,
    // design D1).
    if let Some(code) = session(&cli) {
        std::process::exit(code);
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
        (Some(Command::Run { .. }), Some(doc)) => (
            HostSettings::read_handed(&doc.settings)
                .map(|settings| (settings.held().clone(), Some(settings))),
            Some(doc),
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
    // Record 0119 (design D11): a sign job's record writes the records as
    // the app its environment names; a CodeBuild pool's sign build reads
    // them, and fetches the combination, as it too (design D2 of the change
    // `a-codebuild-fleet-runs-a-queues-checks`).
    let instance = match &cli.command {
        Some(Command::Job {
            command:
                sluice::JobCommand::Record { .. } | sluice::JobCommand::Sign { build: Some(_), .. },
        }) => sluice::job::with_app_from_env(instance).unwrap_or_else(|e| {
            eprintln!("sluice: {e:#}");
            std::process::exit(3);
        }),
        _ => instance,
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
    // A kept `sluice run` follows sluice's host process on a subscription,
    // and binds no repository (the change `sluice-host-answers-every-read`,
    // design D8).
    if let (Some(Command::Run { rounds }), Some(settings)) = (&cli.command, held) {
        let code = keep(&cli, *rounds, settings, started, handed, telemetry.as_ref());
        finish(telemetry);
        std::process::exit(code);
    }
    let opened = repository_dir().and_then(|dir| {
        let program = std::env::current_exe()?;
        let bound = Bound::open_with(&dir, program.clone(), instance, retry.clone())?;
        // Design D17: a command asks sluice's host process for results.
        let bound = Bound {
            state: bound
                .state
                .reading_results(sluice::profiles::records::ResultsFrom::Asked),
            ..bound
        };
        // Record 0120 as amended 2026-11-17 (rule core.10): a worktree's
        // release that no host process answered asks none.
        let bound = match &cli.command {
            Some(Command::Release { name: None, .. }) => sluice::profiles::Bound {
                host_process: Box::new(sluice::host_process::ask::Unasked),
                ..bound
            },
            _ => bound,
        };
        // Record 0119 (design D11; rule core.49): a pool's job is its
        // runner's one work, and holds its run's room in its own process,
        // of no host's bound.
        let bound = match &cli.command {
            Some(Command::Job { .. }) => {
                let common = bound.branch.common_dir()?;
                Bound {
                    tests: bound
                        .tests
                        .with_running(common.join("sluice-job").join("running")),
                    ..bound
                }
            }
            _ => bound,
        };
        let _ = (dir, program);
        Ok(bound)
    });
    let bound = match opened {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("sluice: {e:#}");
            tell_json(json, &e);
            finish(telemetry);
            std::process::exit(3);
        }
    };
    let job_run = matches!(
        cli.command,
        Some(sluice::Command::Job {
            command: sluice::JobCommand::Run { .. }
        })
    );
    if let Err(e) = sluice::stop_on_interrupt().and_then(|()| match job_run {
        true => sluice::stop_on_terminate(),
        false => Ok(()),
    }) {
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
    let ran = run(
        cli,
        bound.seams(),
        &bound,
        &bound.branch,
        &*bound.pulls,
        clock,
        pause,
        &mut out,
    );
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

/// `path` from `folder`, with no `.` in it: the path a read sends.
fn from_folder(folder: &Path, path: &str) -> PathBuf {
    folder
        .join(path)
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

/// The request a read command sends to sluice's host process; `None` for a
/// command that is no read, or a read that the command refuses as it is.
fn read_request(cli: &Cli, folder: &Path) -> Option<sluice::host_process::protocol::Request> {
    use sluice::host_process::protocol::Request;
    let folder_of = |path: &str| from_folder(folder, path);
    Some(match &cli.command {
        None => Request::Home {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            worktree: folder_of(&cli.worktree),
            json: cli.json,
        },
        Some(Command::Status {
            target,
            all,
            json,
            branch,
        }) => Request::Status {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            target: target.clone(),
            all: *all,
            json: *json,
            branch: branch.clone(),
        },
        Some(Command::Overview {
            json, watch: false, ..
        }) => Request::Overview {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            json: *json,
        },
        Some(Command::Submit {
            branch,
            after,
            check: true,
            head,
            worktree,
            json,
            ..
        }) if worktree.len() <= 1 => Request::Forecast {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            branch: branch.clone(),
            worktree: folder_of(worktree.first().map_or(".", String::as_str)),
            head: head.clone(),
            after: after.clone(),
            json: *json,
        },
        Some(Command::Inbox {
            worktree,
            json,
            confirm,
            new,
        }) => Request::Inbox {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            worktree: folder_of(worktree),
            json: *json,
            confirm: *confirm,
            new: *new,
        },
        Some(Command::Previews { json }) => Request::Previews {
            folder: folder.to_path_buf(),
            caller: sluice::caller::here(),
            json: *json,
        },
        _ => return None,
    })
}

/// The request a change command sends to sluice's host process, naming its
/// caller's session and trace context; `None` for a command that is no
/// change (the change `sluice-host-makes-each-change`, design D1).
fn change_request(cli: &Cli, folder: &Path) -> Option<sluice::host_process::protocol::Request> {
    use sluice::host_process::protocol::{Request, SubmitAsk};
    let folder = folder.to_path_buf();
    let caller = sluice::caller::traced();
    Some(match &cli.command {
        Some(Command::Submit {
            branch,
            after,
            priority,
            no_wait,
            subject,
            check: false,
            head,
            status,
            worktree,
            json,
            confirm,
            confirm_landing,
        }) => Request::Submit {
            folder,
            caller,
            submit: SubmitAsk {
                branch: branch.clone(),
                after: after.clone(),
                priority: *priority,
                no_wait: *no_wait,
                subject: subject.clone(),
                head: head.clone(),
                status: *status,
                worktree: worktree.clone(),
                json: *json,
                confirm: *confirm,
                confirm_landing: *confirm_landing,
            },
        },
        Some(Command::Amend {
            id,
            after,
            priority,
            by,
            json,
            branch,
        }) => Request::Amend {
            folder,
            caller,
            id: *id,
            after: after.clone(),
            priority: *priority,
            by: by.clone(),
            json: *json,
            branch: branch.clone(),
        },
        Some(Command::Withdraw {
            id,
            by,
            json,
            branch,
        }) => Request::Withdraw {
            folder,
            caller,
            id: *id,
            by: by.clone(),
            json: *json,
            branch: branch.clone(),
        },
        Some(Command::Confirm {
            keys,
            who,
            worktree,
            json,
        }) => Request::Confirm {
            worktree: from_folder(&folder, worktree),
            folder,
            caller,
            keys: keys.clone(),
            who: who.clone(),
            json: *json,
        },
        Some(Command::Release {
            name: Some(name),
            accept_failure,
            json,
            ..
        }) => Request::Release {
            folder,
            caller,
            name: name.clone(),
            accept_failure: *accept_failure,
            json: *json,
        },
        Some(Command::Branch { command }) => Request::Branch {
            folder,
            caller,
            branch: command.clone(),
        },
        Some(Command::Protect { remove, json }) => Request::Protect {
            folder,
            caller,
            remove: *remove,
            json: *json,
        },
        _ => return None,
    })
}

/// A change, made by sluice's host process, started where none answers:
/// its output printed, and its exit code; `None` for a command that is no
/// change. A waiting submit then follows the wait `sluice host` names, and
/// `sluice step` prints the round `sluice host` took. Where no host process
/// answers, the command says so and exits 3, and changes nothing itself
/// (the change `sluice-host-makes-each-change`, design D1, D2, D5).
fn change(cli: &Cli) -> Option<i32> {
    let folder = match std::env::current_dir() {
        Ok(f) => f,
        Err(e) => {
            let e = anyhow::Error::from(e);
            eprintln!("sluice: {e:#}");
            tell_json(cli.json(), &e);
            return Some(3);
        }
    };
    let step = match &cli.command {
        Some(Command::Step { branch, json }) => Some((branch.clone(), *json)),
        _ => None,
    };
    let request = match step {
        Some(_) => None,
        None => Some(change_request(cli, &folder)?),
    };
    // 124: a command still reads the host's settings in full, and refuses
    // a field it does not know, naming the file and the field.
    if let Err(e) = Instance::read() {
        eprintln!("sluice: {e:#}");
        tell_json(cli.json(), &e);
        return Some(3);
    }
    let did_not = |e: anyhow::Error| {
        let e = e.context("sluice's host process did not answer");
        eprintln!("sluice: {e:#}");
        tell_json(cli.json(), &e);
        Some(3)
    };
    let client = match sluice::host_process::client::Client::host() {
        Ok(c) => c,
        Err(e) => return did_not(e),
    };
    let mut out = std::io::stdout();
    if let Some((branch, json)) = step {
        return match client.step(&folder, &branch) {
            Ok(stepped) => match sluice::say_step(&stepped, json, &mut out) {
                Ok(code) => Some(code),
                Err(e) => did_not(e),
            },
            Err(e) => did_not(e),
        };
    }
    let request = request?;
    let printed = |o: &sluice::host_process::protocol::Output| {
        let mut out = std::io::stdout();
        let _ = out.write_all(o.stdout.as_bytes());
        let _ = out.flush();
        for line in &o.stderr {
            eprintln!("{line}");
        }
    };
    if !matches!(
        request,
        sluice::host_process::protocol::Request::Submit { .. }
    ) {
        return match client.output(&request) {
            Ok(output) => {
                printed(&output);
                Some(output.code)
            }
            Err(e) => did_not(e),
        };
    }
    let submitted = match client.submitted(&request) {
        Ok(s) => s,
        Err(e) => return did_not(e),
    };
    printed(&submitted.output);
    let Some(ask) = submitted.wait else {
        return Some(submitted.output.code);
    };
    // The waiting submit follows `sluice host`, which takes each step of
    // its wait (design D2).
    let every = sluice::tick(cli.command.as_ref());
    let skipped = Skipped::new(every);
    let (clock, pause): (&dyn Clock, &dyn Pause) = match std::env::var(ROUND_SLEEP_VAR).as_deref() {
        Ok("record") => (&skipped, &skipped),
        _ => (&SystemClock, &Sleep(every)),
    };
    let process = sluice::host_process::Socketed::default();
    let followed = sluice::stop_on_interrupt().and_then(|()| {
        sluice::follow_wait(
            &process,
            None,
            folder,
            ask,
            submitted
                .queued
                .then_some(sluice_core::records::State::Queued),
            None,
            clock,
            pause,
            &mut out,
        )
    });
    match followed {
        Ok(code) => Some(code),
        Err(e) => {
            sluice::telemetry::error(&format!("sluice: {e:#}"));
            tell_json(cli.json(), &e);
            Some(3)
        }
    }
}

/// A read, asked of sluice's host process, started where none answers: its
/// output printed, and its exit code; `None` for a command that is no read.
/// Where no host process answers, the command says so and exits 3, and
/// reads nothing itself (the change `sluice-host-answers-every-read`,
/// design D1, D2, D4).
fn read(cli: &Cli) -> Option<i32> {
    let folder = match std::env::current_dir() {
        Ok(f) => f,
        Err(e) => {
            let e = anyhow::Error::from(e);
            eprintln!("sluice: {e:#}");
            tell_json(cli.json(), &e);
            return Some(3);
        }
    };
    let request = read_request(cli, &folder)?;
    // 124: a command still reads the host's settings in full, and refuses
    // a field it does not know, naming the file and the field.
    if let Err(e) = Instance::read() {
        eprintln!("sluice: {e:#}");
        tell_json(cli.json(), &e);
        return Some(3);
    }
    let asked = sluice::host_process::client::Client::host().and_then(|c| c.output(&request));
    match asked {
        Ok(output) => {
            let mut out = std::io::stdout();
            let _ = out.write_all(output.stdout.as_bytes());
            let _ = out.flush();
            for line in &output.stderr {
                eprintln!("{line}");
            }
            Some(output.code)
        }
        Err(e) => {
            let e = e.context("sluice's host process did not answer");
            eprintln!("sluice: {e:#}");
            tell_json(cli.json(), &e);
            Some(3)
        }
    }
}

/// The request a session's command sends to sluice's host process; `None`
/// for any other command (the change `sluice-host-runs-a-sessions-checks`,
/// design D1, D2).
fn session_request(cli: &Cli, folder: &Path) -> Option<sluice::host_process::protocol::Request> {
    use sluice::host_process::protocol::Request;
    let caller = sluice::caller::traced();
    let run_env = sluice::profiles::checks::run_env_here();
    let otel = cli.choices();
    let folder_of = |path: &str| from_folder(folder, path);
    Some(match &cli.command {
        Some(Command::Exec {
            branch,
            command,
            worktree,
            json,
            release,
        }) => Request::Exec {
            folder: folder.to_path_buf(),
            caller,
            run_env,
            otel: otel.clone(),
            branch: branch.clone(),
            command: command.clone(),
            worktree: folder_of(worktree),
            json: *json,
            release: *release,
        },
        Some(Command::Preview {
            branch,
            revision,
            wait,
            json,
        }) => Request::Preview {
            folder: folder.to_path_buf(),
            caller,
            run_env,
            otel: otel.clone(),
            branch: branch.clone(),
            revision: revision.clone(),
            wait: *wait,
            json: *json,
        },
        Some(Command::Equip { branch, at, json }) => Request::Equip {
            folder: folder.to_path_buf(),
            caller,
            run_env,
            otel: otel.clone(),
            branch: branch.clone(),
            at: at.clone(),
            json: *json,
        },
        Some(Command::Release {
            name: None,
            worktree,
            accept_failure,
            json,
        }) => Request::ReleaseWorktree {
            folder: folder.to_path_buf(),
            caller,
            run_env,
            otel: otel.clone(),
            worktree: folder_of(worktree),
            accept_failure: *accept_failure,
            json: *json,
        },
        _ => return None,
    })
}

/// A session's command, run by sluice's host process, started where none
/// answers: what it writes printed as it comes, and its exit code; `None`
/// for any other command. A host process that ends before the command
/// does is said, and the command exits 3 (the change
/// `sluice-host-runs-a-sessions-checks`, design D1, D4).
fn session(cli: &Cli) -> Option<i32> {
    let folder = match std::env::current_dir() {
        Ok(f) => f,
        Err(e) => {
            let e = anyhow::Error::from(e);
            eprintln!("sluice: {e:#}");
            tell_json(cli.json(), &e);
            return Some(3);
        }
    };
    let request = session_request(cli, &folder)?;
    // 124: a command still reads the host's settings in full, and refuses
    // a field it does not know, naming the file and the field.
    if let Err(e) = Instance::read() {
        eprintln!("sluice: {e:#}");
        tell_json(cli.json(), &e);
        return Some(3);
    }
    // Record 0120 as amended 2026-11-17 (rule core.10): a worktree's
    // release asks sluice's host process only where one answers; where
    // none does, it frees what it reads of the records itself, below, and
    // starts none.
    let release = matches!(
        request,
        sluice::host_process::protocol::Request::ReleaseWorktree { .. }
    );
    if let Err(e) = sluice::stop_on_interrupt() {
        eprintln!("sluice: {e:#}");
        tell_json(cli.json(), &e);
        return Some(3);
    }
    let ran = match release {
        // Where no host process has run, or none answers, the release goes
        // on below, making nothing to find one.
        true => match sluice::host_process::client::Client::found() {
            Ok(Some(c)) => match c.command_if_answering(&request) {
                Ok(None) => return None,
                ran => ran.map(|code| code.unwrap_or(0)),
            },
            Ok(None) | Err(_) => return None,
        },
        false => sluice::host_process::client::Client::host().and_then(|c| c.command(&request)),
    };
    Some(match ran {
        Ok(code) => code,
        Err(e) => {
            eprintln!("sluice: {e:#}");
            tell_json(cli.json(), &e);
            3
        }
    })
}

/// `sluice overview --json --watch`, with `--stdin` or without, as a
/// client of a subscription to sluice's host process: its exit; `None` for
/// any other command (the change `sluice-host-answers-every-read`, design
/// D7).
fn watch(cli: &Cli) -> Option<i32> {
    let Some(Command::Overview {
        watch: true, stdin, ..
    }) = &cli.command
    else {
        return None;
    };
    let mut out = std::io::stdout();
    let every = sluice::tick(cli.command.as_ref());
    let skipped = Skipped::new(every);
    let (clock, pause): (&dyn Clock, &dyn Pause) = match std::env::var(ROUND_SLEEP_VAR).as_deref() {
        Ok("record") => (&skipped, &skipped),
        _ => (&SystemClock, &Sleep(every)),
    };
    let watched = sluice::stop_on_interrupt()
        .and_then(|()| Ok(std::env::current_dir()?))
        .and_then(|folder| match stdin {
            true => sluice::subscriber::watch_input(&folder, &mut out),
            false => sluice::subscriber::watch(
                &sluice::host_process::Socketed::default(),
                None,
                folder,
                clock,
                pause,
                &mut out,
            ),
        });
    Some(match watched {
        Ok(code) => code,
        Err(e) => {
            sluice::telemetry::error(&format!("sluice: {e:#}"));
            tell_json(true, &e);
            3
        }
    })
}

/// `sluice wait` as a client of sluice's host process: its exit; `None` for
/// any other command (the change `sluice-host-answers-every-read`, design
/// D8; the change `sluice-host-makes-each-change`, design D2).
fn wait(cli: &Cli) -> Option<i32> {
    let Some(Command::Wait {
        target,
        timeout,
        worktree,
        json,
        confirm,
        confirm_landing,
        branch,
        pull_request,
    }) = &cli.command
    else {
        return None;
    };
    // 124: a command still reads the host's settings in full.
    if let Err(e) = Instance::read() {
        eprintln!("sluice: {e:#}");
        tell_json(*json, &e);
        return Some(3);
    }
    let every = sluice::tick(cli.command.as_ref());
    let skipped = Skipped::new(every);
    let (clock, pause): (&dyn Clock, &dyn Pause) = match std::env::var(ROUND_SLEEP_VAR).as_deref() {
        Ok("record") => (&skipped, &skipped),
        _ => (&SystemClock, &Sleep(every)),
    };
    let deadline = timeout
        .map(|t| clock.now() + chrono::Duration::from_std(t).unwrap_or(chrono::Duration::MAX));
    let mut out = std::io::stdout();
    let process = sluice::host_process::Socketed::default();
    let followed = sluice::stop_on_interrupt()
        .and_then(|()| Ok(std::env::current_dir()?))
        .and_then(|folder| {
            let ask = sluice::host_process::protocol::WaitAsk {
                target: target.clone(),
                branch: branch.clone(),
                pull_request: *pull_request,
                worktree: Some(from_folder(&folder, worktree).display().to_string()),
                json: *json,
                confirm: *confirm,
                confirm_landing: *confirm_landing,
                ..Default::default()
            };
            sluice::follow_wait(
                &process, None, folder, ask, None, deadline, clock, pause, &mut out,
            )
        });
    match followed {
        Ok(code) => Some(code),
        Err(e) => {
            sluice::telemetry::error(&format!("sluice: {e:#}"));
            tell_json(*json, &e);
            Some(3)
        }
    }
}

/// Raise this process's soft limit on open files to its hard limit, as
/// far as the system lets one process open (macOS's
/// `kern.maxfilesperproc`), before sluice reads or starts anything, so a
/// supervisor's low default, launchd's 256 among them, holds neither
/// sluice nor the runs it starts, which inherit the raised limit (135;
/// record 0126; rule core.59). A soft limit already at its hard limit is
/// left as it is. Where the system refuses, sluice goes on under the
/// limit it was given and says so.
fn raise_open_files() {
    if let Err(e) = rlimit::increase_nofile_limit(u64::MAX) {
        eprintln!("sluice: warning: the limit on open files stays as sluice was started with: {e}");
    }
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
    settings: HostSettings,
    started: Started,
    handed: Option<HandOver>,
}

/// A kept `sluice run` of the folder it runs in: its exit. 153 (design
/// D2): it goes through no `run`, and is one span in its caller's trace
/// all the same.
fn keep(
    cli: &Cli,
    rounds: Option<usize>,
    settings: HostSettings,
    started: Started,
    handed: Option<HandOver>,
    telemetry: Option<&Telemetry>,
) -> i32 {
    let every = sluice::tick(cli.command.as_ref());
    let skipped = Skipped::new(every);
    let (clock, pause): (&dyn Clock, &dyn Pause) = match std::env::var(ROUND_SLEEP_VAR).as_deref() {
        Ok("record") => (&skipped, &skipped),
        _ => (&SystemClock, &Sleep(every)),
    };
    let mut out = std::io::stdout();
    let ran = sluice::stop_on_interrupt()
        .and_then(|()| Ok(std::env::current_dir()?))
        .and_then(|dir| {
            let kept = Kept {
                dir,
                settings,
                started,
                handed,
            };
            sluice::telemetry::command(cli.caller.as_ref(), &cli.words(), || {
                keep_running(kept, rounds, telemetry, clock, pause, &mut out)
            })
        });
    match ran {
        Ok(code) => code,
        Err(e) => {
            sluice::telemetry::error(&format!("sluice: {e:#}"));
            3
        }
    }
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

/// A kept `sluice run`, a follower of sluice's host process, one pass at a
/// time, looking at the host's settings between two passes (135; record
/// 0107; rule core.54; design D11). It holds nothing its settings decide
/// but its own telemetry: a change that moves `[telemetry]` it takes up by
/// starting itself afresh, carrying the passes it followed and what it
/// said, once the new settings bind; any other change is nothing for it to
/// take up. A file gone, unreadable or refused leaves it on what it holds,
/// said once.
fn keep_running(
    mut kept: Kept,
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
    let mut following = match kept.handed.take() {
        Some(doc) => {
            eprintln!(
                "sluice: took up the host's settings in {file}: {} changed, in this sluice run \
                 started afresh",
                doc.tables.join(", ")
            );
            Following::handed(rounds, &doc)
        }
        None => {
            let mut following = Following::new(rounds);
            following.start()?;
            following
        }
    };
    // Design D8 of `sluice-host-answers-every-read`: a subscription of its
    // folder as `run`, on the host process's socket, with no binding.
    let process = sluice::host_process::Socketed::default();
    let mut runner =
        sluice::subscriber::Runner::new(&process, None, kept.dir.clone(), following.tick());
    loop {
        if let Driven::Ended(code) = following.next(&mut runner, clock, pause, out)? {
            return Ok(code);
        }
        let refused = |why: &str| {
            sluice::telemetry::warn(&format!(
                "sluice: warning: {file} is refused, so this sluice run keeps the settings it \
                 holds until the file changes: {why}"
            ))
        };
        // A change it takes up only by starting afresh that it could not
        // start afresh for said once with the tables (rule core.54; design
        // D5).
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
                if !tables.iter().any(|t| AFRESH.contains(t)) {
                    // Nothing it holds: the host process takes it up.
                    kept.settings.accept(instance);
                    continue;
                }
                if let Some(t) = telemetry {
                    t.flush();
                }
                let _ = out.flush();
                let why = start_afresh(&kept.started, &following, &tables, text);
                not_afresh(&tables, "it could not start afresh", &format!("{why:#}"));
            }
        }
    }
}

/// Start this `sluice run` afresh in its own process, as it `started`,
/// handing it `text`, the settings it read and bound, the `tables` that
/// changed, and the passes `following` followed and what it said (135;
/// record 0107; design D11). It returns only where the start failed, with
/// why; the run then follows on, holding all it held.
fn start_afresh(
    started: &Started,
    following: &Following,
    tables: &[&str],
    text: String,
) -> anyhow::Error {
    let program = match &started.program {
        Ok(program) => program,
        Err(why) => return anyhow::anyhow!("{why}"),
    };
    let doc = HandOver {
        settings: text,
        tables: tables.iter().map(|t| t.to_string()).collect(),
        passes: following.passes(),
        told: following.told(),
    };
    doc.start(program, &started.args, &started.env)
}

/// The folder the repository is opened from: the one the command runs
/// in, so none takes a folder below a worktree's top for the worktree.
/// `sluice equip`, which equips the worktree holding its folder (147), runs
/// in sluice's host process, which finds the worktree's top.
fn repository_dir() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?)
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
    policy.recording_where_set()
}

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
