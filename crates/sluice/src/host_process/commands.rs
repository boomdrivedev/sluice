//! `sluice host` runs a session's command (the change
//! `sluice-host-runs-a-sessions-checks`, design D1 to D4;
//! `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`,
//! `workers-end-with-their-owner@292e869158d6`,
//! `nothing-detached@f807c7f5fcea`).
//!
//! `sluice exec`, `sluice preview`, `sluice equip` and a worktree's
//! `sluice release` each send their folder, their arguments, their
//! caller's session and their caller's run variables. The connection's task
//! resolves the folder to its repository as a read's is resolved, and runs
//! the command's own code in one call on the pool for blocking work. The
//! call binds the folder as the host holds the repository, with the
//! host's room, and the caller's folder, session and run variables stand
//! for the host process's own on its thread. Each run that the call starts
//! is a worker of `sluice host`.
//!
//! What the command writes on standard output goes to the client as a
//! `stdout` line as it writes it, and each line it says on standard error
//! as a `stderr` line; `output` ends the answer with the exit code. A
//! client that closes its side of the connection before its `output`, as
//! its Ctrl-C does, or that ends, stops the call as Ctrl-C stopped the
//! command: a session's run ends, its slot is given back, and it is
//! recorded as stopped; a wait for a slot stops. What the command says of
//! its stop is sent while the client still reads. A host process that
//! stops ends the call the same way, and sends no `output`: its client
//! reads the closed connection, and says so.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use sluice_seams::RunKind;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::ask::{Here, Holding, Holds, HostProcess};
use super::pass::Report;
use super::protocol::{Answer, Named, Output, Reading, Request, RoomView};
use super::reads::failed;
use super::room::{Asked, Asker, Shared, Why};
use super::Hosting;
use crate::{Cli, Command};

/// How long `sluice exec`'s call waits between two looks at its run: how
/// soon a run's output reaches its client (design D1). Any other command
/// pauses as it paused in its own process, its tick ([`crate::tick`]).
const STREAM: Duration = Duration::from_millis(200);

/// How often a pause looks whether its client closed the connection or
/// the host process stops (design D4).
const LOOK: Duration = Duration::from_millis(20);

/// Whether `request` is a session's command that `sluice host` runs.
pub fn is_command(request: &Request) -> bool {
    matches!(
        request,
        Request::Exec { .. }
            | Request::Preview { .. }
            | Request::Equip { .. }
            | Request::ReleaseWorktree { .. }
    )
}

/// What a command's call is given: the caller's folder, session and run
/// variables, whether it asked for JSON, whether its binding is opened from
/// the top of the worktree that holds the folder, and the command as
/// `sluice` parses it from its arguments.
struct Call {
    folder: PathBuf,
    caller: BTreeMap<String, String>,
    run_env: BTreeMap<String, String>,
    /// The command's own telemetry choices (rule core.53).
    otel: crate::telemetry::Choices,
    json: bool,
    /// `sluice equip` equips the worktree that holds its folder, so its
    /// binding is opened from the worktree's top (147).
    equip: bool,
    cli: Cli,
}

/// The call that `request` asks; `None` for a request that is no
/// session's command.
fn call_of(request: Request) -> Option<Call> {
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    let (folder, caller, run_env, otel, json, equip, command) = match request {
        Request::Exec {
            folder,
            caller,
            run_env,
            otel,
            branch,
            command,
            worktree,
            json,
            release,
        } => (
            folder,
            caller,
            run_env,
            otel,
            json,
            false,
            Command::Exec {
                branch,
                command,
                worktree: path(worktree),
                json,
                release,
            },
        ),
        Request::Preview {
            folder,
            caller,
            run_env,
            otel,
            branch,
            revision,
            wait,
            json,
        } => (
            folder,
            caller,
            run_env,
            otel,
            json,
            false,
            Command::Preview {
                branch,
                revision,
                wait,
                json,
            },
        ),
        Request::Equip {
            folder,
            caller,
            run_env,
            otel,
            branch,
            at,
            json,
        } => (
            folder,
            caller,
            run_env,
            otel,
            json,
            true,
            Command::Equip { branch, at, json },
        ),
        Request::ReleaseWorktree {
            folder,
            caller,
            run_env,
            otel,
            worktree,
            accept_failure,
            json,
        } => (
            folder,
            caller,
            run_env,
            otel,
            json,
            false,
            Command::Release {
                name: None,
                worktree: path(worktree),
                accept_failure,
                json,
            },
        ),
        _ => return None,
    };
    // 153: the command's span nests in its caller's trace.
    let trace = crate::telemetry::Caller::read(caller.iter());
    Some(Call {
        folder,
        caller,
        run_env,
        otel,
        json,
        equip,
        cli: Cli {
            command: Some(command),
            json: false,
            worktree: ".".to_string(),
            otel: None,
            otel_service_name: None,
            otel_logs: None,
            otel_logs_limit: None,
            caller: trace,
        },
    })
}

/// Answer the session's command `request` on its connection (design D1
/// to D4): the lines it writes as they come, then its `output`; nothing
/// more where the client closed the connection or the host process stops.
pub(crate) async fn answer(
    hosting: &Arc<Hosting>,
    request: Request,
    lines: &mut super::Lines,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let said = serde_json::to_string(&request).unwrap_or_default();
    let Some(call) = call_of(request) else {
        return Ok(());
    };
    let json = call.json;
    // `sluice equip` equips the worktree that holds its folder, from any
    // folder of it (147): its binding is opened from the worktree's top.
    let dir = match call.equip {
        true => {
            let folder = call.folder.clone();
            tokio::task::spawn_blocking(move || {
                crate::profiles::worktree_top(&folder)
                    .ok()
                    .flatten()
                    .map_or(folder, PathBuf::from)
            })
            .await
            .unwrap_or_else(|_| call.folder.clone())
        }
        false => call.folder.clone(),
    };
    let repository = match super::reads::resolved(hosting, &dir).await {
        Ok(r) => r,
        Err(words) => return super::send(writer, &Answer::Output(failed(&words, json))).await,
    };
    super::note(hosting, &repository);
    super::log(&format!(
        "sluice host: {}: runs {said}",
        repository.display()
    ));
    let stopped = Arc::new(AtomicBool::new(false));
    let (sender, mut sent) = mpsc::unbounded_channel::<Answer>();
    let mut running = {
        let (hosting, stopped) = (hosting.clone(), stopped.clone());
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            run(&hosting, &repository, &dir, call, sender, stopped, runtime)
        })
    };
    // Whether the client closed its side, whether it reads no more, and
    // whether the host process stops.
    let (mut closed, mut gone, mut stopping, mut drained) = (false, false, false, false);
    let ran = loop {
        tokio::select! {
            answer = sent.recv(), if !drained => match answer {
                Some(answer) => {
                    if !gone && super::send(writer, &answer).await.is_err() {
                        gone = true;
                        stopped.store(true, Ordering::SeqCst);
                    }
                }
                None => drained = true,
            },
            ran = &mut running => break ran,
            () = super::closed(lines), if !closed => {
                closed = true;
                stopped.store(true, Ordering::SeqCst);
            }
            () = hosting.stop.cancelled(), if !stopping => {
                stopping = true;
                stopped.store(true, Ordering::SeqCst);
            }
        }
    };
    if gone || stopping {
        return Ok(());
    }
    while let Ok(answer) = sent.try_recv() {
        super::send(writer, &answer).await?;
    }
    let output = ran.unwrap_or_else(|e| failed(&format!("the command ended: {e}"), json));
    super::send(writer, &Answer::Output(output)).await
}

/// The command's one call (design D1, D2): bind the folder as the host
/// holds the repository, and run the command's code with the caller's
/// folder, session and run variables standing for the host process's own,
/// each line it writes sent as it comes. The binding is dropped before it
/// returns.
fn run(
    hosting: &Arc<Hosting>,
    repository: &Path,
    dir: &Path,
    call: Call,
    sender: mpsc::UnboundedSender<Answer>,
    stopped: Arc<AtomicBool>,
    runtime: tokio::runtime::Handle,
) -> Output {
    let Call {
        folder,
        caller,
        run_env,
        otel,
        json,
        cli,
        ..
    } = call;
    let said = sender.clone();
    let say: Box<dyn FnMut(&str)> = Box::new(move |line| {
        let _ = said.send(Answer::Stderr(line.to_string()));
    });
    let mut out = Streamed {
        sender,
        held: Vec::new(),
    };
    // Rule core.53: a command's own telemetry choices choose for it
    // alone; where it chose nothing, the host's telemetry holds.
    let telemetry = (!otel.is_empty()).then(|| {
        let settings =
            crate::instance::Instance::read().unwrap_or_else(|_| hosting.settings.instance());
        crate::telemetry::Settings::choose(
            otel,
            settings.telemetry.as_ref(),
            &crate::profiles::checks::hostname(),
            std::env::vars(),
        )
    });
    let traced = |f: &mut dyn FnMut() -> Result<i32>| match &telemetry {
        Some(settings) => crate::telemetry::scoped(settings, f),
        None => f(),
    };
    let mut call = Some((caller, run_env, cli, runtime, stopped));
    let ran = crate::telemetry::streaming(say, || {
        traced(&mut || {
            let (caller, run_env, cli, runtime, stopped) =
                call.take().expect("the command runs once");
            crate::caller::in_folder(caller, Some(folder.clone()), || {
                crate::profiles::checks::with_run_env(run_env, || -> Result<i32> {
                    let bound = bind(hosting, repository, dir, runtime, stopped.clone())?;
                    let every = match &cli.command {
                        Some(Command::Exec { .. }) => STREAM,
                        command => crate::tick(command.as_ref()),
                    };
                    let pause = Stopping { stopped, every };
                    crate::run(
                        cli,
                        bound.seams(),
                        &bound,
                        &bound.branch,
                        &*bound.pulls,
                        &sluice_core::SystemClock,
                        &pause,
                        &mut out,
                    )
                })
            })
        })
    });
    let _ = out.flush();
    match ran {
        Ok(code) => Output {
            stdout: String::new(),
            stderr: Vec::new(),
            code,
            pass: None,
        },
        Err(e) => failed(&format!("{e:#}"), json),
    }
}

/// The binding a command's call runs in (design D1, D3): the host's
/// settings as the file holds them now, its runs' parts held of the host's
/// room, its results from the host process's index, and the host process
/// as the call asks it.
fn bind(
    hosting: &Arc<Hosting>,
    repository: &Path,
    dir: &Path,
    runtime: tokio::runtime::Handle,
    stopped: Arc<AtomicBool>,
) -> Result<crate::profiles::Bound> {
    // A command reads the host's settings as its command read them as it
    // started (rule core.54).
    let settings =
        crate::instance::Instance::read().unwrap_or_else(|_| hosting.settings.instance());
    let bound = crate::profiles::Bound::open_holding(
        dir,
        std::env::current_exe()?,
        settings,
        Rc::new(crate::retry::Policy::command().recording_where_set()),
        &hosting.tokens,
    )?;
    Ok(crate::profiles::Bound {
        tests: bound
            .tests
            .with_room(crate::profiles::checks::RoomHandle::Here(
                hosting.room.clone(),
            )),
        state: bound
            .state
            .reading_results(crate::profiles::records::ResultsFrom::Indexed),
        host_process: Box::new(Running {
            hosting: hosting.clone(),
            repository: repository.to_path_buf(),
            runtime,
            stopped,
        }),
        ..bound
    })
}

/// What a command writes on standard output, sent to its client a line
/// at a time, and what is left of a line when it flushes.
struct Streamed {
    sender: mpsc::UnboundedSender<Answer>,
    held: Vec<u8>,
}

impl Streamed {
    fn send(&mut self, upto: usize) {
        let text: Vec<u8> = self.held.drain(..upto).collect();
        if !text.is_empty() {
            let _ = self
                .sender
                .send(Answer::Stdout(String::from_utf8_lossy(&text).into_owned()));
        }
    }
}

impl Write for Streamed {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.held.extend_from_slice(buf);
        if let Some(at) = self.held.iter().rposition(|b| *b == b'\n') {
            self.send(at + 1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.send(self.held.len());
        Ok(())
    }
}

/// A command's pause of `every`, ended at once where its client closed the
/// connection or the host process stops, as Ctrl-C ended the command's own
/// (design D4).
struct Stopping {
    stopped: Arc<AtomicBool>,
    every: Duration,
}

impl crate::Pause for Stopping {
    fn pause(&self) -> bool {
        let until = std::time::Instant::now() + self.every;
        while std::time::Instant::now() < until {
            if self.stopped.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(LOOK);
        }
        !self.stopped.load(Ordering::SeqCst)
    }
}

/// The host process as a command's call asks it (design D1, D3): a slot
/// of its own room, and the repository's scheduler told of a change.
struct Running {
    hosting: Arc<Hosting>,
    repository: PathBuf,
    runtime: tokio::runtime::Handle,
    stopped: Arc<AtomicBool>,
}

impl HostProcess for Running {
    fn asked_only(&self) -> bool {
        super::asked_only()
    }

    fn note(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    /// The repository's scheduler told of the change, in place of the
    /// `serve` that the command sent (design D1).
    fn serve(&self, _: &Here<'_, '_>) -> Result<()> {
        let _entered = self.runtime.enter();
        super::note(&self.hosting, &self.repository);
        self.hosting.tasks.told(&self.repository);
        let task = self.hosting.tasks.handle(&self.repository);
        if !super::asked_only() {
            task.wake();
        }
        Ok(())
    }

    fn reach(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn step(&self, _: &Here<'_, '_>, _: &str) -> Result<serde_json::Value> {
        Err(anyhow!("a session's command runs no round"))
    }

    fn pass(&self, _: &Here<'_, '_>) -> Result<Report> {
        Err(anyhow!("a session's command runs no pass"))
    }

    fn read(&self, _: &Here<'_, '_>, _: bool) -> Result<Reading> {
        Err(anyhow!("a session's command asks no reading"))
    }

    /// A slot of the host's own room, in the order of the run bound (design
    /// D3): its wait counted from this ask, and let go where it is held,
    /// or where the call stops.
    fn hold<'a>(
        &'a self,
        _: &Here<'a, '_>,
        _: RunKind,
        _: &str,
        branch: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        let room = self.hosting.room.clone();
        let waiter = room.lock().wait(&self.repository, Some(branch));
        let changed = room.subscribe();
        Ok(Box::new(Waiting {
            room,
            repository: self.repository.clone(),
            branch: branch.to_string(),
            waiter: Some(waiter),
            said: None,
            changed,
            runtime: self.runtime.clone(),
            stopped: self.stopped.clone(),
        }))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(Some((
            self.hosting.named.clone(),
            super::view_of(&self.hosting.room)?,
        )))
    }

    fn subscribe<'a>(
        &'a self,
        _: Option<&'a Here<'a, 'a>>,
        _: Vec<super::protocol::Added>,
    ) -> Result<Box<dyn super::ask::Subscription + 'a>> {
        Err(anyhow!("a session's command subscribes to nothing"))
    }

    fn pass_of(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &dyn sluice_core::Clock,
    ) -> Result<Report> {
        Err(anyhow!("a session's command runs no pass"))
    }

    fn wait(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        Err(anyhow!("a session's command asks no wait"))
    }
}

/// A session's or a preview's wait for a slot of the host's room (design
/// D3): it asks the room again each time the room changes and when a
/// combination's wait lapses, says why it waits once for each change, and
/// lets its wait go when it is held or dropped.
struct Waiting {
    room: Shared,
    repository: PathBuf,
    branch: String,
    /// Its wait in the room, until it is held.
    waiter: Option<u64>,
    said: Option<Why>,
    changed: tokio::sync::watch::Receiver<u64>,
    runtime: tokio::runtime::Handle,
    stopped: Arc<AtomicBool>,
}

impl Waiting {
    fn let_go(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            self.room.lock().let_go(waiter);
            self.room.changed();
        }
    }
}

impl Holding for Waiting {
    fn next(&mut self, wait: Duration) -> Result<Option<Holds>> {
        let Some(waiter) = self.waiter else {
            bail!("the slot was held already");
        };
        if self.stopped.load(Ordering::SeqCst) {
            self.let_go();
            return Ok(None);
        }
        self.changed.borrow_and_update();
        let asked = self.room.lock().ask(
            &Asker::Session(waiter),
            &self.repository,
            Some(&self.branch),
            Instant::now(),
        );
        match asked {
            Ok(Asked::Held(id)) => {
                self.let_go();
                return Ok(Some(Holds::Held(self.room.part(RunKind::Session, id))));
            }
            Ok(Asked::Refused(why)) => {
                if self.said != Some(why) {
                    self.said = Some(why);
                    return Ok(Some(Holds::Waiting(why)));
                }
            }
            Err(e) => {
                self.let_go();
                return Err(e);
            }
        }
        let lapse = self.room.lock().next_lapse();
        let now = Instant::now();
        let until = lapse.map_or(wait, |l| wait.min(l.saturating_duration_since(now)));
        let mut changed = self.changed.clone();
        self.runtime.block_on(async {
            let _ = tokio::time::timeout(until, changed.changed()).await;
        });
        Ok(None)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.let_go();
    }
}
