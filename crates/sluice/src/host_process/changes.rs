//! `sluice host` makes each change (the change
//! `sluice-host-makes-each-change`, design D1, D2;
//! `residents-do-the-network-and-git-work@a2d68cf95ffe`).
//!
//! A change names its client's folder, its command's arguments and its
//! caller's session and trace context. The connection's task resolves the
//! folder to its repository, as a read's is, and the change is one call
//! on the pool for blocking work: the call binds the repository as the
//! host holds it, takes the branch's lock where the command took it, runs
//! the command's own code into a buffer with the caller's variables and
//! folder in place of the host process's own, keeps what it says on
//! standard error, and drops the binding before it returns. A change is
//! joined with no other ask, and takes no slot of `[runs] max`
//! (`runs-max-counts-checks-only@2409f7ec2f4f`; design D6). Once the call
//! ends, `sluice host` tells the repository's scheduler that a change was
//! made, as `serve` told it. `sluice previews` is answered so too: its
//! listing frees each preview whose checkout is gone, which a read's
//! binding, which writes nothing, could not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;

use super::protocol::{Answer, Output, Reading, Request, Submitted};
use super::reads::{answering, failed, resolved};
use super::Hosting;
use crate::{Cli, Command};

/// Whether `request` is a change that `sluice host` makes.
pub fn is_change(request: &Request) -> bool {
    matches!(
        request,
        Request::Submit { .. }
            | Request::Amend { .. }
            | Request::Withdraw { .. }
            | Request::Confirm { .. }
            | Request::Release { .. }
            | Request::Branch { .. }
            | Request::Protect { .. }
            | Request::Previews { .. }
    )
}

/// The folder a change names, its caller, and whether it asked for JSON.
fn parts(request: &Request) -> (PathBuf, BTreeMap<String, String>, bool) {
    match request {
        Request::Submit {
            folder,
            caller,
            submit,
        } => (folder.clone(), caller.clone(), submit.json),
        Request::Amend {
            folder,
            caller,
            json,
            ..
        }
        | Request::Withdraw {
            folder,
            caller,
            json,
            ..
        }
        | Request::Confirm {
            folder,
            caller,
            json,
            ..
        }
        | Request::Release {
            folder,
            caller,
            json,
            ..
        }
        | Request::Protect {
            folder,
            caller,
            json,
            ..
        }
        | Request::Previews {
            folder,
            caller,
            json,
        } => (folder.clone(), caller.clone(), *json),
        Request::Branch {
            folder,
            caller,
            branch,
        } => (folder.clone(), caller.clone(), branch_json(branch)),
        _ => (PathBuf::new(), BTreeMap::new(), false),
    }
}

fn branch_json(command: &crate::BranchCommand) -> bool {
    match command {
        crate::BranchCommand::Create { json, .. }
        | crate::BranchCommand::Delete { json, .. }
        | crate::BranchCommand::Accept { json, .. } => *json,
    }
}

/// The answer `output` makes of a change: a submit's is `submitted`,
/// which names no wait where it ended.
fn answered(submit: bool, output: Output) -> Answer {
    match submit {
        true => Answer::Submitted(Submitted {
            output,
            wait: None,
            queued: false,
        }),
        false => Answer::Output(output),
    }
}

/// Make the change `request` (design D1).
pub(crate) async fn answer(hosting: &Arc<Hosting>, request: Request) -> Answer {
    let (folder, _, json) = parts(&request);
    let submit = matches!(request, Request::Submit { .. });
    let repository = match resolved(hosting, &folder).await {
        Ok(r) => r,
        Err(words) => return answered(submit, failed(&words, json)),
    };
    // One line for each change, which names what it made.
    super::log(&format!(
        "sluice host: {}: change {}",
        repository.display(),
        serde_json::to_string(&request).unwrap_or_default()
    ));
    let held = hosting.clone();
    let made_in = repository.clone();
    let runtime = tokio::runtime::Handle::current();
    let changing = hosting.tasks.changing(&repository);
    let made = tokio::task::spawn_blocking(move || {
        // The repository's pass in flight ends first, and none begins
        // until the change is made.
        let _whole = changing.write().unwrap_or_else(|e| e.into_inner());
        make(&held, &made_in, request, runtime)
    })
    .await
    .unwrap_or_else(|e| answered(submit, failed(&format!("the change ended: {e}"), json)));
    // In place of `serve`: the repository noted, once the change is made,
    // so the watch that takes it in reads it after the change; no reading
    // begun before the change is current; and the repository is passed at
    // once, or held for the next ask where passes are taken only when
    // asked.
    super::note(hosting, &repository);
    hosting.tasks.told(&repository);
    let task = hosting.tasks.handle(&repository);
    if !super::asked_only() {
        task.wake();
    }
    made
}

/// What a change's call made.
enum Made {
    Code(i32),
    Submitted(crate::Submitted),
}

/// The change's one call (design D1): bind the folder as the host holds
/// the repository, run the command's code with the caller's variables and
/// folder, and give what it printed, what it said on standard error and
/// its exit code. The binding is dropped before it returns.
fn make(
    hosting: &Hosting,
    repository: &Path,
    request: Request,
    runtime: tokio::runtime::Handle,
) -> Answer {
    let (folder, caller, json) = parts(&request);
    let submit = matches!(request, Request::Submit { .. });
    // 153: the trace context the command was started under, which its
    // span nests in and its submission keeps (design D1).
    let traced = crate::telemetry::Caller::read(caller.iter());
    let within = Some(folder.clone());
    let (ran, mut stderr) = crate::telemetry::capturing(|| {
        crate::caller::in_folder(caller, within, || -> Result<(Made, Vec<u8>)> {
            // 134: the retry policy its command ran under: a waiting submit
            // waits out a spent allowance, and one that does not wait stops
            // on it at once.
            let policy = match &request {
                Request::Submit { submit, .. } if submit.no_wait => {
                    crate::retry::Policy::command().unpaused()
                }
                Request::Submit { submit, .. } if !submit.status => {
                    crate::retry::Policy::waiting(None)
                }
                _ => crate::retry::Policy::command(),
            };
            let bound = crate::profiles::Bound::open_holding(
                &folder,
                std::env::current_exe()?,
                // A change reads the host's settings as its command read
                // them as it started; a round takes a change up only
                // between rounds (rule core.54).
                crate::instance::Instance::read().unwrap_or_else(|_| hosting.settings.instance()),
                Rc::new(policy.recording_where_set()),
                &hosting.tokens,
            )?;
            let bound = crate::profiles::Bound {
                state: bound
                    .state
                    .reading_results(crate::profiles::records::ResultsFrom::Indexed),
                host_process: answering(hosting, repository, Reading::default(), runtime),
                ..bound
            };
            let clock = super::clock::TokioClock::new();
            let mut out = Vec::new();
            let cli = Cli {
                caller: traced,
                ..cli_of(request)
            };
            let made = match submit {
                true => {
                    // 136: a submit's own read of its request is what its
                    // wait's first step follows on from.
                    bound.pulls.remember(hosting.followed(repository));
                    let made = crate::submit_run(
                        cli,
                        bound.seams(),
                        &bound,
                        &bound.branch,
                        &*bound.pulls,
                        &clock,
                        &mut out,
                    );
                    hosting.keep_followed(repository, bound.pulls.remembered());
                    Made::Submitted(made?)
                }
                false => Made::Code(crate::run(
                    cli,
                    bound.seams(),
                    &bound,
                    &bound.branch,
                    &*bound.pulls,
                    &clock,
                    &NoPause,
                    &mut out,
                )?),
            };
            Ok((made, out))
        })
    });
    let (made, out) = match ran {
        Ok(ran) => ran,
        Err(e) => {
            let mut output = failed(&format!("{e:#}"), json);
            stderr.append(&mut output.stderr);
            output.stderr = stderr;
            return answered(submit, output);
        }
    };
    let stdout = String::from_utf8_lossy(&out).into_owned();
    match made {
        Made::Code(code) => Answer::Output(Output {
            stdout,
            stderr,
            code,
        }),
        Made::Submitted(crate::Submitted::Done(code)) => answered(
            true,
            Output {
                stdout,
                stderr,
                code,
            },
        ),
        Made::Submitted(crate::Submitted::Wait { ask, queued }) => Answer::Submitted(Submitted {
            output: Output {
                stdout,
                stderr,
                code: 0,
            },
            wait: Some(ask),
            queued,
        }),
    }
}

/// A change takes no pause.
struct NoPause;

impl crate::Pause for NoPause {
    fn pause(&self) -> bool {
        false
    }
}

/// The command a change runs, as `sluice` parses it from its client's
/// arguments.
fn cli_of(request: Request) -> Cli {
    let command = match request {
        Request::Submit { submit, .. } => Some(Command::Submit {
            branch: submit.branch,
            after: submit.after,
            priority: submit.priority,
            no_wait: submit.no_wait,
            subject: submit.subject,
            check: false,
            head: submit.head,
            status: submit.status,
            worktree: submit.worktree,
            json: submit.json,
            confirm: submit.confirm,
            confirm_landing: submit.confirm_landing,
        }),
        Request::Amend {
            id,
            after,
            priority,
            by,
            json,
            branch,
            ..
        } => Some(Command::Amend {
            id,
            after,
            priority,
            by,
            json,
            branch,
        }),
        Request::Withdraw {
            id,
            by,
            json,
            branch,
            ..
        } => Some(Command::Withdraw {
            id,
            by,
            json,
            branch,
        }),
        Request::Confirm {
            keys,
            who,
            worktree,
            json,
            ..
        } => Some(Command::Confirm {
            keys,
            who,
            worktree: worktree.to_string_lossy().into_owned(),
            json,
        }),
        Request::Release {
            name,
            accept_failure,
            json,
            ..
        } => Some(Command::Release {
            name: Some(name),
            worktree: ".".into(),
            accept_failure,
            json,
        }),
        Request::Branch { branch, .. } => Some(Command::Branch { command: branch }),
        Request::Protect { remove, json, .. } => Some(Command::Protect { remove, json }),
        Request::Previews { json, .. } => Some(Command::Previews { json }),
        _ => None,
    };
    Cli {
        command,
        json: false,
        worktree: ".".into(),
        otel: None,
        otel_service_name: None,
        otel_logs: None,
        otel_logs_limit: None,
        caller: None,
    }
}
