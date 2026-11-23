//! `sluice host` answers each read from the state that it keeps (the
//! change `sluice-host-answers-every-read`, design D1 to D5;
//! `reads-go-through-the-resident@69134e5825d7`,
//! `one-read-in-flight@e99acda774d0`).
//!
//! A read names its client's folder and its command's arguments. The
//! connection's task resolves the folder to its repository, joined by the
//! host's map of folders, and waits for a current reading of the
//! repository within the read's bound. Then the read is one call on the
//! pool for blocking work, joined by the repository's map of reads: the
//! call binds the folder as the host holds it, runs the command's own code
//! into a buffer, keeps what it says on standard error, and drops the
//! binding before it returns. The call never goes to the repository's
//! worker, so a round never holds it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use sluice_seams::RunKind;

use super::ask::{Here, Holding, HostProcess};
use super::joined::Joined;
use super::pass::Report;
use super::protocol::{Named, Output, Reading, Request, RoomView};
use super::room::Shared;
use super::tasks::Tasks;
use super::Hosting;
use crate::{Cli, Command};

/// What the host process joins of the reads: a folder's repository, by the
/// folder, and each read, by the repository and the request (design D5).
pub(crate) struct Reads {
    pub folders: Joined<PathBuf, Result<PathBuf, String>>,
    pub asks: std::sync::Mutex<std::collections::BTreeMap<PathBuf, Joined<String, Output>>>,
}

impl Reads {
    pub fn new(tracker: tokio_util::task::TaskTracker) -> Reads {
        Reads {
            folders: Joined::new(tracker),
            asks: Default::default(),
        }
    }

    /// `repository`'s map of reads.
    fn of(
        &self,
        repository: &Path,
        tracker: &tokio_util::task::TaskTracker,
    ) -> Joined<String, Output> {
        self.asks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(repository.to_path_buf())
            .or_insert_with(|| Joined::new(tracker.clone()))
            .clone()
    }
}

/// Whether `request` is a read that `sluice host` answers with `output`.
pub fn is_read(request: &Request) -> bool {
    matches!(
        request,
        Request::Status { .. }
            | Request::Overview { .. }
            | Request::Home { .. }
            | Request::Forecast { .. }
            | Request::Inbox { .. }
    )
}

/// The folder a read names, and whether it asked for JSON.
fn folder_of(request: &Request) -> (PathBuf, bool) {
    match request {
        Request::Status { folder, json, .. }
        | Request::Overview { folder, json, .. }
        | Request::Home { folder, json, .. }
        | Request::Forecast { folder, json, .. }
        | Request::Inbox { folder, json, .. } => (folder.clone(), *json),
        _ => (PathBuf::new(), false),
    }
}

/// The caller's session variables that a read names.
fn caller_of(request: &Request) -> std::collections::BTreeMap<String, String> {
    match request {
        Request::Status { caller, .. }
        | Request::Overview { caller, .. }
        | Request::Home { caller, .. }
        | Request::Forecast { caller, .. }
        | Request::Inbox { caller, .. } => caller.clone(),
        _ => Default::default(),
    }
}

/// The repository that holds `folder`, as its common git directory, read
/// as a command bound it: the words a command gave where it could not bind
/// it (design D2).
pub fn resolve(folder: &Path) -> Result<PathBuf, String> {
    crate::profiles::local_refs::LocalRefs::open(folder)
        .and_then(|r| r.common_dir())
        .map_err(|e| format!("{e:#}"))
}

/// The output of a command that failed itself (exit 3): its words on
/// standard error, and as a JSON document on standard output where the
/// client asked for JSON, as `main` gave them.
pub fn failed(words: &str, json: bool) -> Output {
    let stdout = match json {
        true => format!(
            "{}\n",
            crate::told::document(crate::told::Kind::Internal, words, &[])
        ),
        false => String::new(),
    };
    Output {
        stdout,
        stderr: vec![format!("sluice: {words}")],
        code: crate::INTERNAL,
    }
}

/// The repository that holds `folder`, joined by the host's map of
/// folders (design D2, D5).
pub(crate) async fn resolved(hosting: &Hosting, folder: &Path) -> Result<PathBuf, String> {
    let resolving = folder.to_path_buf();
    hosting
        .reads
        .folders
        .ask(folder.to_path_buf(), None, None, move || async move {
            tokio::task::spawn_blocking(move || resolve(&resolving))
                .await
                .unwrap_or_else(|e| Err(format!("resolving the folder: {e}")))
        })
        .await
        .unwrap_or_else(|| Err("resolving the folder ended with no answer".to_string()))
}

/// Answer the read `request` (design D1 to D5).
pub(crate) async fn answer(hosting: &Arc<Hosting>, request: Request) -> Output {
    let (folder, json) = folder_of(&request);
    let tracker = hosting.connected.clone();
    let resolved = resolved(hosting, &folder).await;
    let repository = match resolved {
        Ok(r) => r,
        Err(words) => return failed(&words, json),
    };
    super::note(hosting, &repository);
    let reading = hosting
        .tasks
        .read(&repository, super::READ_TICK, super::read_for(), false)
        .await;
    let key = serde_json::to_string(&request).unwrap_or_default();
    let changed = hosting.tasks.told_at(&repository);
    let held = hosting.clone();
    let said = key.clone();
    hosting
        .reads
        .of(&repository, &tracker)
        .ask(key, changed, None, move || async move {
            // One line for each call, which the asks that join it share.
            super::log(&format!(
                "sluice host: {}: read {said}",
                repository.display()
            ));
            let runtime = tokio::runtime::Handle::current();
            tokio::task::spawn_blocking(move || {
                render(&held, &repository, &folder, request, reading, runtime)
            })
            .await
            .unwrap_or_else(|e| failed(&format!("the read ended: {e}"), json))
        })
        .await
        .unwrap_or_else(|| failed("the read ended with no answer", json))
}

/// `repository`'s overview document, as `sluice overview --json` prints it
/// in its main folder, read after a reading that ended at `after`, joined
/// with each one-shot overview of that folder (design D5, D6); `None` where
/// the read printed no document.
pub(crate) async fn document(
    hosting: &Arc<Hosting>,
    repository: PathBuf,
    after: super::joined::Moment,
) -> Option<serde_json::Value> {
    let folder = super::tasks::open_dir(&repository);
    let request = Request::Overview {
        folder: folder.clone(),
        caller: Default::default(),
        json: true,
    };
    let reading = hosting.tasks.last_reading(&repository);
    let key = serde_json::to_string(&request).ok()?;
    let changed = Some(
        hosting
            .tasks
            .told_at(&repository)
            .map_or(after, |t| t.max(after)),
    );
    let held = hosting.clone();
    let tracker = hosting.connected.clone();
    let output = hosting
        .reads
        .of(&repository, &tracker)
        .ask(key, changed, None, move || async move {
            let runtime = tokio::runtime::Handle::current();
            tokio::task::spawn_blocking(move || {
                render(&held, &repository, &folder, request, reading, runtime)
            })
            .await
            .unwrap_or_else(|e| failed(&format!("the read ended: {e}"), true))
        })
        .await?;
    serde_json::from_str(output.stdout.trim()).ok()
}

/// The command a read runs, as `sluice` parses it from its client's
/// arguments; `None` for a request that is no read.
fn cli_of(request: Request) -> Option<Cli> {
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    let (command, json, worktree) = match request {
        Request::Status {
            target,
            all,
            json,
            branch,
            ..
        } => (
            Some(Command::Status {
                target,
                all,
                json,
                branch,
            }),
            false,
            ".".to_string(),
        ),
        Request::Overview { json, .. } => (
            Some(Command::Overview {
                json,
                watch: false,
                stdin: false,
            }),
            false,
            ".".to_string(),
        ),
        Request::Home { worktree, json, .. } => (None, json, path(worktree)),
        Request::Forecast {
            branch,
            worktree,
            head,
            after,
            json,
            ..
        } => (
            Some(Command::Submit {
                branch,
                after,
                priority: 0,
                no_wait: false,
                subject: Vec::new(),
                check: true,
                head,
                status: false,
                worktree: vec![path(worktree)],
                json,
                confirm: false,
                confirm_landing: false,
            }),
            false,
            ".".to_string(),
        ),
        Request::Inbox {
            worktree,
            json,
            confirm,
            new,
            ..
        } => (
            Some(Command::Inbox {
                worktree: path(worktree),
                json,
                confirm,
                new,
            }),
            false,
            ".".to_string(),
        ),
        _ => return None,
    };
    Some(Cli {
        command,
        json,
        worktree,
        otel: None,
        otel_service_name: None,
        otel_logs: None,
        otel_logs_limit: None,
        caller: None,
    })
}

/// A read takes no pause.
struct NoPause;

impl crate::Pause for NoPause {
    fn pause(&self) -> bool {
        false
    }
}

/// The read's one call (design D3): bind `folder` as the host holds it,
/// run the command's code with `reading` as the host process's answer,
/// and give what it printed, what it said on standard error and its exit
/// code. The binding is dropped before it returns.
fn render(
    hosting: &Hosting,
    repository: &Path,
    folder: &Path,
    request: Request,
    reading: Reading,
    runtime: tokio::runtime::Handle,
) -> Output {
    let json = folder_of(&request).1;
    let caller = caller_of(&request);
    let Some(cli) = cli_of(request) else {
        return failed("the request is no read", json);
    };
    let (ran, mut stderr) = crate::telemetry::capturing(|| -> Result<(i32, Vec<u8>)> {
        let bound = bind_read(hosting, repository, folder, reading, runtime)?;
        let clock = super::clock::TokioClock::new();
        let mut out = Vec::new();
        let code = crate::caller::as_caller(caller, || {
            crate::run(
                cli,
                bound.seams(),
                &bound,
                &bound.branch,
                &*bound.pulls,
                &clock,
                &NoPause,
                &mut out,
            )
        })?;
        Ok((code, out))
    });
    match ran {
        Ok((code, out)) => Output {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr,
            code,
        },
        Err(e) => {
            let mut output = failed(&format!("{e:#}"), json);
            stderr.append(&mut output.stderr);
            output.stderr = stderr;
            output
        }
    }
}

/// The binding a read's call reads `folder` with, as the host holds the
/// repository (design D3): the host's settings as the file holds them
/// now, the results from the host process's index, what the last ask read
/// as last fetched, and the host process as the read takes it.
fn bind_read(
    hosting: &Hosting,
    repository: &Path,
    folder: &Path,
    reading: Reading,
    runtime: tokio::runtime::Handle,
) -> Result<crate::profiles::Bound> {
    bind_read_with(
        hosting,
        repository,
        folder,
        reading,
        runtime,
        crate::retry::Policy::command(),
    )
}

/// [`bind_read`], every call to a remote under `policy` (134).
fn bind_read_with(
    hosting: &Hosting,
    repository: &Path,
    folder: &Path,
    reading: Reading,
    runtime: tokio::runtime::Handle,
    policy: crate::retry::Policy,
) -> Result<crate::profiles::Bound> {
    // A read reads the host's settings as its command read them as it
    // started; a round takes a change up only between rounds (rule
    // core.54).
    let settings =
        crate::instance::Instance::read().unwrap_or_else(|_| hosting.settings.instance());
    let bound = crate::profiles::Bound::open_holding(
        folder,
        std::env::current_exe()?,
        settings,
        Rc::new(policy.recording_where_set()),
        &hosting.tokens,
    )?;
    let bound = crate::profiles::Bound {
        state: bound
            .state
            .reading_results(crate::profiles::records::ResultsFrom::Indexed)
            .seeded(reading.stale.clone()),
        host_process: answering(hosting, repository, reading, runtime),
        ..bound
    };
    Ok(bound)
}

/// The host process as a call of `sluice host` on `repository` asks it:
/// `reading`, a reading afresh where a forecast asks one, and the room as
/// it holds it; nothing it is told, and no socket of its own.
pub(crate) fn answering(
    hosting: &Hosting,
    repository: &Path,
    reading: Reading,
    runtime: tokio::runtime::Handle,
) -> Box<dyn HostProcess> {
    Box::new(Answering {
        room: hosting.room.clone(),
        named: hosting.named.clone(),
        reading,
        tasks: hosting.tasks.clone(),
        repository: repository.to_path_buf(),
        runtime,
    })
}

/// Answer a wait's step (design D8): the folder resolved and the
/// repository read as a read's are, then one call on the pool for
/// blocking work that binds the folder and takes the step.
pub(crate) async fn wait(
    hosting: &Arc<Hosting>,
    folder: PathBuf,
    caller: std::collections::BTreeMap<String, String>,
    ask: super::protocol::WaitAsk,
) -> super::protocol::Answer {
    use super::protocol::{Answer, WaitStep};
    let json = ask.json;
    let repository = match resolved(hosting, &folder).await {
        Ok(r) => r,
        Err(words) => return Answer::Waited(WaitStep::Ended(failed(&words, json))),
    };
    super::note(hosting, &repository);
    let reading = hosting
        .tasks
        .read(&repository, super::READ_TICK, super::read_for(), false)
        .await;
    let held = hosting.clone();
    let runtime = tokio::runtime::Handle::current();
    // 134: a forge read of a pull request's step is tried again until the
    // wait's own time runs out, as the wait tried it (the change
    // `sluice-host-makes-each-change`, design D2).
    let until = ask
        .deadline
        .as_deref()
        .and_then(|d| d.parse::<chrono::DateTime<chrono::Utc>>().ok())
        .map(|d| {
            let left = (d - chrono::Utc::now()).to_std().unwrap_or_default();
            std::time::Instant::now() + left
        });
    let stepped = tokio::task::spawn_blocking(move || {
        let (ran, stderr) = crate::telemetry::capturing(|| {
            let within = Some(folder.clone());
            crate::caller::in_folder(
                caller,
                within,
                || -> Result<(crate::WaitStepped, Vec<u8>)> {
                    let policy = crate::retry::Policy::waiting(until);
                    let bound =
                        bind_read_with(&held, &repository, &folder, reading, runtime, policy)?;
                    // 136: the requests the waits on the repository follow,
                    // as the last step left what GitHub said of them.
                    bound.pulls.remember(held.followed(&repository));
                    let clock = super::clock::TokioClock::new();
                    let s = sluice_core::Sluice::new(bound.seams(), &clock)
                        .as_peer(crate::exec::Host::peer(&bound))
                        .holding();
                    let mut out = Vec::new();
                    let stepped =
                        crate::wait_step(&s, &bound, &*bound.pulls, &clock, &ask, &mut out);
                    held.keep_followed(&repository, bound.pulls.remembered());
                    Ok((stepped?, out))
                },
            )
        });
        (ran, stderr)
    })
    .await;
    let (ran, mut stderr) = match stepped {
        Ok(r) => r,
        Err(e) => {
            return Answer::Waited(WaitStep::Ended(failed(
                &format!("the wait ended: {e}"),
                json,
            )))
        }
    };
    match ran {
        Ok((crate::WaitStepped::Ended(code), out)) => Answer::Waited(WaitStep::Ended(Output {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr,
            code,
        })),
        Ok((crate::WaitStepped::Follow(own, follows), out)) => Answer::Waited(WaitStep::Follow {
            own,
            follows,
            said: String::from_utf8_lossy(&out).into_owned(),
        }),
        Ok((crate::WaitStepped::Pull(pull), out)) => Answer::Waited(WaitStep::Pull {
            pull: Box::new(pull),
            said: String::from_utf8_lossy(&out).into_owned(),
        }),
        Ok((crate::WaitStepped::Pulls(pulls), out)) => Answer::Waited(WaitStep::Pulls {
            pulls: Box::new(pulls),
            said: String::from_utf8_lossy(&out).into_owned(),
        }),
        Err(e) => {
            let mut output = failed(&format!("{e:#}"), json);
            stderr.append(&mut output.stderr);
            output.stderr = stderr;
            Answer::Waited(WaitStep::Ended(output))
        }
    }
}

/// The host process as a read's call asks it (design D3): the reading the
/// connection's task waited for, a reading afresh where a forecast asks
/// one, and the room as it holds it.
struct Answering {
    room: Shared,
    named: Named,
    reading: Reading,
    tasks: Tasks,
    repository: PathBuf,
    runtime: tokio::runtime::Handle,
}

impl HostProcess for Answering {
    fn asked_only(&self) -> bool {
        super::asked_only()
    }

    fn note(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn serve(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn reach(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn step(&self, _: &Here<'_, '_>, _: &str) -> Result<serde_json::Value> {
        Err(anyhow!("a read runs no round"))
    }

    fn pass(&self, _: &Here<'_, '_>) -> Result<Report> {
        Err(anyhow!("a read runs no pass"))
    }

    /// The reading the read waited for; with `afresh`, a reading begun
    /// after this ask, as a forecast's second ask has it (129 as amended
    /// 2026-11-19).
    fn read(&self, _: &Here<'_, '_>, afresh: bool) -> Result<Reading> {
        if !afresh {
            return Ok(self.reading.clone());
        }
        Ok(self.runtime.block_on(self.tasks.read(
            &self.repository,
            super::READ_TICK,
            super::read_for(),
            true,
        )))
    }

    fn hold<'a>(
        &'a self,
        _: &Here<'a, '_>,
        _: RunKind,
        _: &str,
        _: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        Err(anyhow!("a read holds no room"))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(Some((self.named.clone(), super::view_of(&self.room)?)))
    }

    fn subscribe<'a>(
        &'a self,
        _: Option<&'a Here<'a, 'a>>,
        _: Vec<super::protocol::Added>,
    ) -> Result<Box<dyn super::ask::Subscription + 'a>> {
        Err(anyhow!("a read subscribes to nothing"))
    }

    fn pass_of(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &dyn sluice_core::Clock,
    ) -> Result<Report> {
        Err(anyhow!("a read runs no pass"))
    }

    fn wait(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        Err(anyhow!("a read asks no wait"))
    }
}
