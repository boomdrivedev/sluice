//! What a command and the host process say to each other on its socket
//! (design D2): lines of JSON. A command says `hello` with its version, the
//! process answers with its own, and the command asks one request on the
//! connection; a follow and a held room keep their connection open.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::RunKind;

use super::pass::Report;
use super::room::{Share, Turn, Waiting, Why};

/// What a command says first on a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub hello: String,
}

/// The process as it answers `hello`, and as `host/version` names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub version: String,
    pub pid: u32,
    /// When it started, RFC 3339, to the millisecond, so an image started
    /// afresh in place within the second its pid's last image started is
    /// told apart (design D4 of `sluice-runs-as-a-daemon`).
    pub since: String,
    /// Whether it passes only when asked (design D14): a follower of it
    /// asks a pass at each of its own ticks, whatever its own setting.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asked_only: bool,
    /// Whether it is `sluice daemon`, which never ends for being idle and
    /// hands over by starting afresh in place (137; design D1 of
    /// `sluice-runs-as-a-daemon`). An older process says nothing of it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub daemon: bool,
    /// The executable it runs, resolved through every link: what a service
    /// manager's activation compares with the program it installs (the
    /// change `sluice-host-owns-its-workers`, design D5). An older process
    /// says nothing of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
}

/// What follows the host process's passes of a repository (D7, D11), and
/// how a subscription follows a folder (the change
/// `sluice-host-answers-every-read`, design D6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowKind {
    /// `sluice overview --watch`: its repository is read each second while
    /// work is in play, and on a file event otherwise.
    #[default]
    Watch,
    /// `sluice wait`, a waiting submit or `wt submit`.
    Wait,
    /// `sluice run`, which prints each pass.
    Run,
}

/// A folder that `subscribe` names, and how the client follows it (the
/// change `sluice-host-answers-every-read`, design D6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Added {
    pub folder: PathBuf,
    #[serde(rename = "as", default)]
    pub kind: FollowKind,
}

/// A line a client sends on a subscription: `add` or `remove` (design D6).
pub type SubscriptionAsk = super::subscription::Ask<FollowKind>;

/// A line `sluice host` sends on a subscription: a folder's `document`, a
/// folder's `pass`, or an error (design D6).
pub type SubscriptionLine = super::subscription::Sent<serde_json::Value, Report>;

/// A request, after `hello`.
///
/// The reads (`status`, `overview`, `home`, `forecast` and `inbox`) each
/// name the client's folder, an absolute path, and the command's own
/// arguments; `sluice host` answers each with `output` (the change
/// `sluice-host-answers-every-read`, design D1, D2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "kebab-case")]
pub enum Request {
    /// `sluice status`, in each form.
    Status {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// `sluice overview` without `--watch`.
    Overview {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// The home view, `sluice` with no command, of the worktree at
    /// `worktree`.
    Home {
        folder: PathBuf,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice submit --check`: a forecast of `worktree`'s head, or of the
    /// branch `head`, on `branch`.
    Forecast {
        folder: PathBuf,
        branch: String,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        after: Vec<u64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice inbox` of the worktree at `worktree`.
    Inbox {
        folder: PathBuf,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        confirm: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        new: bool,
    },
    /// Serve `folders`, and the folders that the client adds after on the
    /// connection, each with its newest overview (design D6).
    Subscribe {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        folders: Vec<Added>,
    },
    /// The repository has work: hold it, and pass it at once.
    Serve { repository: PathBuf },
    /// One round of `branch`, answered with the step's document (D10).
    Step { repository: PathBuf, branch: String },
    /// A pass of the repository now, answered once it ends (D14). `at`,
    /// RFC 3339, is the asker's time: where passes are taken only when
    /// asked, the pass is at that time, so the host process's time is its
    /// askers', however many ask.
    Pass {
        repository: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
    /// The results `wants` names, as `branch`'s gate reads them, answered
    /// from the host process's index of the repository on the connection's
    /// own thread (D17).
    Results {
        repository: PathBuf,
        branch: String,
        wants: sluice_seams::ResultAsk,
    },
    /// A session's or a preview's room, the connection kept while it
    /// waits and while it is held (D12).
    Hold {
        kind: RunKind,
        repository: PathBuf,
        place: String,
        /// 154: the branch its run is held to; `None` from an older sluice,
        /// whose run counts toward the bound under no branch (design D6 of
        /// `each-submission-shows-its-wait-and-its-test`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// Hand over, for a newer sluice (D8). `program` is the asker's
    /// executable, resolved through every link, which the host process
    /// starts afresh in place as; an older command sends none, and the host
    /// process starts afresh as its own (design D3 of
    /// `sluice-runs-as-a-daemon` and of `sluice-host-owns-its-workers`).
    /// `ends` says that the asker takes the host's place itself, as a
    /// daemon does: the host process ends, and its runs end with it. A host
    /// process of an older sluice ends on any `hand-over`.
    HandOver {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        program: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ends: bool,
    },
}

/// A repository's reading, as the host process holds it for a read (129;
/// design D16): whether it was current, when it ended, and the words a
/// remote refused it with, where one did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    pub current: bool,
    /// When it ended, RFC 3339; `None` where none has.
    #[serde(default)]
    pub ended: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// Whether the look made it, which writes no reader's document: a
    /// one-shot overview then renders the records as the host holds them,
    /// never a document an earlier pass wrote (design D16).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub looked: bool,
    /// The branches the reading's last ask read as last fetched, each with
    /// the remote's refusal: what a read's call binds the repository with,
    /// as the host holds it (the change `sluice-host-answers-every-read`,
    /// design D3). It stays in the host process.
    #[serde(skip)]
    pub stale: std::collections::BTreeMap<String, sluice_seams::ReadRefused>,
}

/// What a held room's command says once its run has begun.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    pub place: String,
    pub run_dir: PathBuf,
}

/// The room as the process holds it, for a person to read (150, 71;
/// record 0123).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomView {
    pub bound: usize,
    pub reserve: usize,
    pub queue: usize,
    pub sessions: usize,
    pub waiting: Waiting,
    pub turn: Turn,
    /// Each repository's runs under test and runs waiting, by its common
    /// git directory.
    #[serde(default)]
    pub repositories: BTreeMap<PathBuf, Share>,
}

/// A read's answer: what the command prints on standard output, the lines
/// it says on standard error, and its exit code (design D1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub stdout: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stderr: Vec<String>,
    pub code: i32,
}

/// An answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Answer {
    /// A read's output (design D1).
    Output(Output),
    /// Done, for a serve or a hand-over.
    Ok,
    /// A step's document, as `sluice step --json` prints it.
    Step(serde_json::Value),
    /// A pass's report: for a pass, and each pass of a followed
    /// repository.
    Pass(Report),
    /// A held room's wait, with why.
    Waiting(Why),
    /// The room is the asker's.
    Held,
    /// The results asked for, one JSON document per line (D17).
    Results(String),
    /// The request could not be answered, in these words.
    Error(String),
}

/// Write `value` as one line, flushed.
pub fn send(to: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    to.write_all(&line)?;
    to.flush()?;
    Ok(())
}

/// The next line read as `T`; an error where the other end closed.
pub fn receive<T: for<'de> Deserialize<'de>>(from: &mut impl BufRead) -> Result<T> {
    let mut line = String::new();
    if from.read_line(&mut line)? == 0 {
        bail!("the host process closed the connection");
    }
    serde_json::from_str(line.trim_end()).with_context(|| format!("reading {:?}", line.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The request as an older sluice's host process knows it: `hand-over`
    /// with no field.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "ask", rename_all = "kebab-case")]
    enum Older {
        HandOver,
    }

    /// host-daemon, "A newer sluice's command has a daemon start afresh in
    /// place" (design D3 of `sluice-runs-as-a-daemon`): a `hand-over`
    /// naming the asker's executable reads as a hand-over naming it; one
    /// from an older command, which names none, reads as one naming none;
    /// and a host process of an older sluice reads the one naming it as its
    /// own bare `hand-over`, so it ends on it as before.
    #[test]
    fn a_hand_over_naming_a_program_reads_as_a_hand_over() {
        let named = Request::HandOver {
            program: Some(PathBuf::from("/x/sluice")),
            ends: false,
        };
        let line = serde_json::to_string(&named).unwrap();
        assert_eq!(line, r#"{"ask":"hand-over","program":"/x/sluice"}"#);
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), named);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"ask":"hand-over"}"#).unwrap(),
            Request::HandOver {
                program: None,
                ends: false
            }
        );
        assert_eq!(
            serde_json::to_string(&Request::HandOver {
                program: None,
                ends: false
            })
            .unwrap(),
            r#"{"ask":"hand-over"}"#
        );
        assert_eq!(
            serde_json::from_str::<Older>(&line).unwrap(),
            Older::HandOver
        );
        // A daemon's ask to end reads as one to an older host process,
        // which ends on it.
        let ending = Request::HandOver {
            program: Some(PathBuf::from("/x/sluice")),
            ends: true,
        };
        let line = serde_json::to_string(&ending).unwrap();
        assert_eq!(
            line,
            r#"{"ask":"hand-over","program":"/x/sluice","ends":true}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), ending);
        assert_eq!(
            serde_json::from_str::<Older>(&line).unwrap(),
            Older::HandOver
        );
    }

    /// design D1, D2: each read names its folder and its command's
    /// arguments, and reads back as it was sent; a read's output reads back.
    #[test]
    fn the_reads_read_back() {
        let reads = [
            Request::Status {
                folder: "/w/feat".into(),
                target: Some("main".into()),
                all: false,
                json: true,
                branch: None,
            },
            Request::Overview {
                folder: "/w/feat".into(),
                json: false,
            },
            Request::Home {
                folder: "/w/feat".into(),
                worktree: "/w/feat".into(),
                json: true,
            },
            Request::Forecast {
                folder: "/w/feat".into(),
                branch: "main".into(),
                worktree: "/w/feat".into(),
                head: None,
                after: vec![4],
                json: true,
            },
            Request::Inbox {
                folder: "/w/feat".into(),
                worktree: "/w/feat".into(),
                json: false,
                confirm: true,
                new: false,
            },
        ];
        for read in reads {
            let mut line = Vec::new();
            send(&mut line, &read).unwrap();
            assert_eq!(receive::<Request>(&mut line.as_slice()).unwrap(), read);
        }
        let line = br#"{"ask":"status","folder":"/w/feat","target":"main","json":true}"#;
        assert_eq!(
            serde_json::from_slice::<Request>(line).unwrap(),
            Request::Status {
                folder: "/w/feat".into(),
                target: Some("main".into()),
                all: false,
                json: true,
                branch: None,
            }
        );
        let output = Answer::Output(Output {
            stdout: "{}\n".into(),
            stderr: vec!["sluice: warning: late".into()],
            code: 0,
        });
        let line = serde_json::to_string(&output).unwrap();
        assert_eq!(
            line,
            r#"{"output":{"stdout":"{}\n","stderr":["sluice: warning: late"],"code":0}}"#
        );
        assert_eq!(serde_json::from_str::<Answer>(&line).unwrap(), output);
    }

    /// design D6: `subscribe` with its folders, its `add` and `remove`
    /// lines, and its `document` and `pass` lines read back.
    #[test]
    fn a_subscription_reads_back() {
        let subscribe = Request::Subscribe {
            folders: vec![Added {
                folder: "/w/feat".into(),
                kind: FollowKind::Wait,
            }],
        };
        let line = serde_json::to_string(&subscribe).unwrap();
        assert_eq!(
            line,
            r#"{"ask":"subscribe","folders":[{"folder":"/w/feat","as":"wait"}]}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), subscribe);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"ask":"subscribe"}"#).unwrap(),
            Request::Subscribe { folders: vec![] }
        );
        let add: SubscriptionAsk = serde_json::from_str(r#"{"add":"/w/feat"}"#).unwrap();
        assert_eq!(
            add,
            SubscriptionAsk::Add {
                add: "/w/feat".into(),
                kind: None
            }
        );
        let add: SubscriptionAsk = serde_json::from_str(r#"{"add":"/w/main","as":"run"}"#).unwrap();
        assert_eq!(
            add,
            SubscriptionAsk::Add {
                add: "/w/main".into(),
                kind: Some(FollowKind::Run)
            }
        );
        let remove: SubscriptionAsk = serde_json::from_str(r#"{"remove":"/w/feat"}"#).unwrap();
        assert_eq!(
            remove,
            SubscriptionAsk::Remove {
                remove: "/w/feat".into()
            }
        );
        let document: SubscriptionLine = serde_json::from_str(
            r#"{"document":{"folder":"/w/feat","overview":{"schema":"sluice/overview@1"}}}"#,
        )
        .unwrap();
        assert_eq!(
            document,
            SubscriptionLine::Document(super::super::subscription::Document {
                folder: "/w/feat".into(),
                overview: serde_json::json!({"schema": "sluice/overview@1"}),
            })
        );
        let pass = SubscriptionLine::Pass(super::super::subscription::Pass {
            folder: "/w/main".into(),
            report: Report {
                lines: vec!["main: #4 landed as 9c1e2f0".into()],
                ..Report::default()
            },
        });
        let line = serde_json::to_string(&pass).unwrap();
        assert_eq!(
            serde_json::from_str::<SubscriptionLine>(&line).unwrap(),
            pass
        );
    }

    /// A process's `hello` from before a daemon says nothing of one, and
    /// reads as no daemon (design D1 of `sluice-runs-as-a-daemon`).
    #[test]
    fn a_hello_that_names_no_daemon_is_no_daemon() {
        let named: Named =
            serde_json::from_str(r#"{"version":"0.71.0","pid":7,"since":"2026-11-07T00:00:00Z"}"#)
                .unwrap();
        assert!(!named.daemon);
    }

    /// 154, design D6 of `each-submission-shows-its-wait-and-its-test`: a
    /// hold naming `main` reads back naming it, and one an older sluice
    /// wrote, with no branch, reads naming none.
    #[test]
    fn a_hold_names_its_branch_and_an_older_one_none() {
        let named = Request::Hold {
            kind: RunKind::Session,
            repository: PathBuf::from("/host/repo.git"),
            place: "unit/a".into(),
            branch: Some("main".into()),
        };
        let mut line = Vec::new();
        send(&mut line, &named).unwrap();
        let read: Request = receive(&mut line.as_slice()).unwrap();
        assert_eq!(read, named);
        let older = b"{\"ask\":\"hold\",\"kind\":\"session\",\
                      \"repository\":\"/host/repo.git\",\"place\":\"unit/a\"}\n";
        let read: Request = receive(&mut older.as_slice()).unwrap();
        assert_eq!(
            read,
            Request::Hold {
                kind: RunKind::Session,
                repository: PathBuf::from("/host/repo.git"),
                place: "unit/a".into(),
                branch: None,
            }
        );
    }
}
