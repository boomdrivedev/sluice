//! What a command asks `sluice host`, as the binary asks it: the request
//! each read, change and session command sends, built from the command
//! line and the folder it runs in, and a read sent and its answer
//! printed (the changes `sluice-host-answers-every-read`,
//! `sluice-host-makes-each-change` and `sluice-host-runs-a-sessions-checks`,
//! design D1). The folder, the host's settings and the way to `sluice
//! host` are the caller's: the binary's own, or a test's.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::host_process::client::Client;
use crate::host_process::protocol::Request;
use crate::{Cli, Command};

/// `path` from `folder`, with no `.` in it: the path a read sends.
pub fn from_folder(folder: &Path, path: &str) -> PathBuf {
    folder
        .join(path)
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

/// The request a read command sends to sluice's host process; `None` for a
/// command that is no read, or a read that the command refuses as it is.
pub fn read_request(cli: &Cli, folder: &Path) -> Option<crate::host_process::protocol::Request> {
    use crate::host_process::protocol::Request;
    let folder_of = |path: &str| from_folder(folder, path);
    Some(match &cli.command {
        None => Request::Home {
            folder: folder.to_path_buf(),
            caller: crate::caller::here(),
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
            caller: crate::caller::here(),
            target: target.clone(),
            all: *all,
            json: *json,
            branch: branch.clone(),
        },
        Some(Command::Overview {
            json, watch: false, ..
        }) => Request::Overview {
            folder: folder.to_path_buf(),
            caller: crate::caller::here(),
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
            caller: crate::caller::here(),
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
            caller: crate::caller::here(),
            worktree: folder_of(worktree),
            json: *json,
            confirm: *confirm,
            new: *new,
        },
        Some(Command::Previews { json }) => Request::Previews {
            folder: folder.to_path_buf(),
            caller: crate::caller::here(),
            json: *json,
        },
        _ => return None,
    })
}

/// The request a change command sends to sluice's host process, naming its
/// caller's session and trace context; `None` for a command that is no
/// change (the change `sluice-host-makes-each-change`, design D1).
pub fn change_request(cli: &Cli, folder: &Path) -> Option<crate::host_process::protocol::Request> {
    use crate::host_process::protocol::{Request, SubmitAsk};
    let folder = folder.to_path_buf();
    let caller = crate::caller::traced();
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

/// The request a session's command sends to sluice's host process; `None`
/// for any other command (the change `sluice-host-runs-a-sessions-checks`,
/// design D1, D2).
pub fn session_request(cli: &Cli, folder: &Path) -> Option<crate::host_process::protocol::Request> {
    use crate::host_process::protocol::Request;
    let caller = crate::caller::traced();
    let run_env = crate::profiles::checks::run_env_here();
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

/// The read `request` sent to `sluice host` through `client`, what it
/// answered printed, its output on `out` and its words on stderr, and its
/// exit; where `sluice host` did not answer, said so, exit 3, reading
/// nothing itself (the change `sluice-host-answers-every-read`, design D1,
/// D2, D4).
pub fn read(client: Result<Client>, request: &Request, json: bool, out: &mut dyn Write) -> i32 {
    match client.and_then(|c| c.output(request)) {
        Ok(output) => {
            let _ = out.write_all(output.stdout.as_bytes());
            let _ = out.flush();
            for line in &output.stderr {
                crate::telemetry::say(line);
            }
            output.code
        }
        Err(e) => {
            let e = e.context("sluice's host process did not answer");
            crate::telemetry::say(&format!("sluice: {e:#}"));
            tell_json(json, &e);
            3
        }
    }
}

/// sluice failing itself (exit 3), told on stdout as well when the reader
/// asked for JSON and waits for it there.
pub fn tell_json(json: bool, e: &anyhow::Error) {
    if json {
        let d = crate::told::document(crate::told::Kind::Internal, &format!("{e:#}"), &[]);
        println!("{d}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap()
    }

    /// A path a read sends is its folder's, with no `.` in it.
    #[test]
    fn a_path_from_the_folder_holds_no_dot() {
        let folder = Path::new("/work/repo");
        assert_eq!(from_folder(folder, "."), PathBuf::from("/work/repo"));
        assert_eq!(
            from_folder(folder, "./a/./b"),
            PathBuf::from("/work/repo/a/b")
        );
    }

    /// A status asks `sluice host` for the folder it runs in, as its
    /// command line gives it; a command that is no read asks no read, and
    /// a forecast of several worktrees is no read either.
    #[test]
    fn a_read_asks_for_its_folder_and_a_change_asks_no_read() {
        let folder = Path::new("/work/repo");
        match read_request(&cli(&["status", "main", "--all", "--json"]), folder) {
            Some(Request::Status {
                folder: f,
                target,
                all,
                json,
                ..
            }) => {
                assert_eq!(f, folder);
                assert_eq!(target.as_deref(), Some("main"));
                assert!(all && json);
            }
            other => panic!("{other:?}"),
        }
        assert!(read_request(&cli(&["withdraw", "3"]), folder).is_none());
        let several = cli(&[
            "submit",
            "main",
            "--check",
            "--worktree",
            "a",
            "--worktree",
            "b",
        ]);
        assert!(read_request(&several, folder).is_none());
        assert!(change_request(&cli(&["withdraw", "3"]), folder).is_some());
        assert!(session_request(&cli(&["status"]), folder).is_none());
    }
}
