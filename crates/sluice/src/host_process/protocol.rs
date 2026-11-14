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
    /// When it started, RFC 3339.
    pub since: String,
    /// Whether it passes only when asked (design D14): a follower of it
    /// asks a pass at each of its own ticks, whatever its own setting.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asked_only: bool,
}

/// What follows the host process's passes of a repository (D7, D11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowKind {
    /// `sluice overview --watch`: its repository is read every second.
    Watch,
    /// `sluice wait`, a waiting submit or `wt submit`.
    Wait,
    /// `sluice run`, which prints each pass.
    Run,
}

/// A request, after `hello`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "kebab-case")]
pub enum Request {
    /// The repository has work: hold it, and pass it at once.
    Serve { repository: PathBuf },
    /// One round of `branch`, answered with the step's document (D10).
    Step { repository: PathBuf, branch: String },
    /// Keep the repository read while the connection is open, each pass's
    /// report written down it.
    Follow {
        repository: PathBuf,
        kind: FollowKind,
    },
    /// A pass of the repository now, answered once it ends (D14).
    Pass { repository: PathBuf },
    /// A session's or a preview's room, the connection kept while it
    /// waits and while it is held (D12).
    Hold {
        kind: RunKind,
        repository: PathBuf,
        place: String,
    },
    /// The bound, what counts against it, the runs waiting and whose turn
    /// is next.
    Room,
    /// End, for a newer sluice (D8).
    HandOver,
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

/// An answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Answer {
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
    Room(RoomView),
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
