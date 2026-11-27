//! What a command and the host process say to each other on its socket
//! (design D2): lines of JSON. A command says `hello` with its version, the
//! process answers with its own, and the command asks one request on the
//! connection; a subscription and a session's command keep their connection
//! open, the command's output sent as it comes.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::pass::Report;
use super::room::{Share, Turn, Waiting};

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

/// A submission a wait follows: its number and its branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Followed {
    pub id: u64,
    pub branch: String,
}

/// What a wait asks of `sluice host` (the change
/// `sluice-host-answers-every-read`, design D8): its command's arguments,
/// the submission it waits on and the one it follows, where it knows them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitAsk {
    /// `sluice wait`'s target: a submission's number or a branch.
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
    /// The waiting worktree, whose outcome the wait hands over; none for a
    /// wait that hands over nothing, a submit by name's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub json: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm_landing: bool,
    /// The submission it waits on, once it knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<Followed>,
    /// The submission it follows now, its own or what carries its work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following: Option<Followed>,
    /// A waiting submit's copy, which its record's document carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<sluice_core::ops::CopyDiffers>,
    /// The pull request it follows, as its last step left it (the change
    /// `sluice-host-makes-each-change`, design D2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<Box<PullWait>>,
    /// The act of several worktrees it waits on, in their order (125;
    /// design D2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupWait>,
    /// The act of several pull requests it waits on, as its last step left
    /// it (125; design D2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulls: Option<Box<PullGroupWait>>,
    /// The wait stops with this exit, out of time or stopped: `sluice host`
    /// says what it waits on as it stands, and reads no forge for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<i32>,
    /// When the wait's `--timeout` runs out, RFC 3339: a forge read of its
    /// step is tried again until then (134).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    /// The wait's time as it asks this step. Where `sluice host` passes
    /// only when asked, the step is at this time, as a pass is at the time
    /// of the command that asks it (design D14).
    pub at: chrono::DateTime<chrono::Utc>,
}

/// A pull request a wait follows, as its last step left it (C.5; the
/// change `sluice-host-makes-each-change`, design D2): `sluice host` reads
/// the forge for each step, and the wait carries what it said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullWait {
    pub branch: String,
    pub number: u64,
    /// The worktree whose branch opened it, which its outcome is handed
    /// over to; none for a branch named (124).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_to: Option<String>,
    /// Record 0100 D4: the submission the wait follows the request for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows_for: Option<sluice_core::records::Submission>,
    /// Record 0100 D4: a waiting submit's and `sluice wait <branch>`'s own
    /// is the first submission of the request it reads.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub takes_own: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<sluice_core::records::Submission>,
    /// Its state and the lines beneath it, as last said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, Vec<String>)>,
    /// 137: where it stands on the host that takes it in, as last read,
    /// and as last said, where that is another host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub there: Option<crate::There>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said_there: Option<String>,
    /// The submission it was last read taken in as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken: Option<u64>,
    /// 134: the end of the spent allowance the wait has said, which no
    /// later step says again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub told: Option<chrono::DateTime<chrono::Utc>>,
}

/// One worktree of an act a wait follows (125).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupItem {
    pub id: u64,
    /// The worktree, as the act named it.
    pub worktree: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<sluice_core::ops::CopyDiffers>,
    /// What carries its work, where its wait follows another (record 0100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following: Option<Followed>,
}

/// An act of several worktrees a wait follows, each in turn (125).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupWait {
    pub branch: String,
    pub items: Vec<GroupItem>,
}

/// One pull request of an act a wait follows (125).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullItem {
    pub number: u64,
    /// The worktree, as the act named it.
    pub worktree: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<sluice_core::ops::CopyDiffers>,
    /// The submission the act queued for it, where it took its group
    /// whole, and what was queued ahead of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ahead: Vec<u64>,
    /// How it was decided, once a step read it so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<crate::Followed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, Vec<String>)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub there: Option<crate::There>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said_there: Option<String>,
}

/// An act of several pull requests a wait follows, as its last step left
/// it (125).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullGroupWait {
    pub branch: String,
    pub items: Vec<PullItem>,
    /// 134: the end of the spent allowance the wait has said, which no
    /// later step says again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub told: Option<chrono::DateTime<chrono::Utc>>,
}

/// What `sluice submit` asks of `sluice host`: its command's arguments
/// (the change `sluice-host-makes-each-change`, design D2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitAsk {
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<u64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub priority: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_wait: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub status: bool,
    /// The worktrees as the command names them, each from its folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktree: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub json: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm_landing: bool,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// How `sluice host` answers a submit (design D2): what the submit
/// printed, and, for a waiting submit, the wait it follows next.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Submitted {
    pub output: Output,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitAsk>,
    /// Whether the submit said its submission queued, so the wait says it
    /// no more.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
}

/// How `sluice host` answers a wait's step (design D8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaitStep {
    /// The wait ends: its report, the hand-over recorded, and its exit.
    Ended(Output),
    /// The wait follows `follows`, its own submission `own` or what carries
    /// its work, as the records hold each now.
    Follow {
        own: sluice_core::records::Submission,
        follows: sluice_core::records::Submission,
        /// What the step said, as `sluice wait` said it, such as what
        /// carries its work.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        said: String,
        /// The lines the step said on standard error.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        stderr: Vec<String>,
    },
    /// The wait follows a pull request, as this step left it, and says
    /// `said` (the change `sluice-host-makes-each-change`, design D2).
    Pull {
        pull: Box<PullWait>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        said: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        stderr: Vec<String>,
    },
    /// The wait follows an act's pull requests, as this step left them
    /// (125; design D2).
    Pulls {
        pulls: Box<PullGroupWait>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        said: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        stderr: Vec<String>,
    },
}

/// A request, after `hello`.
///
/// The reads (`status`, `overview`, `home`, `forecast`, `inbox` and
/// `previews`) each name the client's folder, an absolute path, and the
/// command's own arguments; `sluice host` answers each with `output` (the
/// change `sluice-host-answers-every-read`, design D1, D2). So does each
/// change (`submit`, `amend`, `withdraw`, `confirm`, `release`, `branch`
/// and `protect`), naming its caller's session and trace context too (the
/// change `sluice-host-makes-each-change`, design D1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "kebab-case")]
pub enum Request {
    /// `sluice status`, in each form.
    Status {
        folder: PathBuf,
        /// The caller's session variables (the change
        /// `sluice-host-answers-every-read`, design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
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
        /// The caller's session variables (the change
        /// `sluice-host-answers-every-read`, design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// The home view, `sluice` with no command, of the worktree at
    /// `worktree`.
    Home {
        folder: PathBuf,
        /// The caller's session variables (the change
        /// `sluice-host-answers-every-read`, design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice submit --check`: a forecast of `worktree`'s head, or of the
    /// branch `head`, on `branch`.
    Forecast {
        folder: PathBuf,
        /// The caller's session variables (the change
        /// `sluice-host-answers-every-read`, design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
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
        /// The caller's session variables (the change
        /// `sluice-host-answers-every-read`, design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        confirm: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        new: bool,
    },
    /// A wait's step: where its submission stands, as `wait` (design D8).
    Wait {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(flatten)]
        wait: WaitAsk,
    },
    /// `sluice exec` from `folder`: a session's checks, or the command
    /// after `--`, in the worktree `worktree`, which `sluice host` runs as
    /// its worker, its output sent as it comes (the change
    /// `sluice-host-runs-a-sessions-checks`, design D1 to D4).
    Exec {
        folder: PathBuf,
        /// The caller's session variables (design D1).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        /// The variables of the caller's environment that a run keeps
        /// (design D2).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        run_env: BTreeMap<String, String>,
        /// The command's own telemetry choices, `--otel` among them, which
        /// choose its telemetry alone (rule core.53).
        #[serde(default, skip_serializing_if = "crate::telemetry::Choices::is_empty")]
        otel: crate::telemetry::Choices,
        branch: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        command: Vec<String>,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        release: bool,
    },
    /// `sluice preview` from `folder`: its command a worker of `sluice
    /// host`, which stays after the connection closes, until its release
    /// (design D1).
    Preview {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        run_env: BTreeMap<String, String>,
        /// The command's own telemetry choices, `--otel` among them, which
        /// choose its telemetry alone (rule core.53).
        #[serde(default, skip_serializing_if = "crate::telemetry::Choices::is_empty")]
        otel: crate::telemetry::Choices,
        branch: String,
        revision: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        wait: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice equip` from `folder`, its steps workers of `sluice host`,
    /// the worktree's top found from the folder (design D1).
    Equip {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        run_env: BTreeMap<String, String>,
        /// The command's own telemetry choices, `--otel` among them, which
        /// choose its telemetry alone (rule core.53).
        #[serde(default, skip_serializing_if = "crate::telemetry::Choices::is_empty")]
        otel: crate::telemetry::Choices,
        branch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice release` of the worktree `worktree`, its release command a
    /// worker of `sluice host` (design D1).
    ReleaseWorktree {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        run_env: BTreeMap<String, String>,
        /// The command's own telemetry choices, `--otel` among them, which
        /// choose its telemetry alone (rule core.53).
        #[serde(default, skip_serializing_if = "crate::telemetry::Choices::is_empty")]
        otel: crate::telemetry::Choices,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        accept_failure: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice previews`.
    Previews {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice submit`, but `--check`, which is a forecast (design D2).
    Submit {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(flatten)]
        submit: SubmitAsk,
    },
    /// `sluice amend`.
    Amend {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<Vec<u64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<i64>,
        by: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// `sluice withdraw`.
    Withdraw {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        id: u64,
        by: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// `sluice confirm` for the worktree at `worktree`.
    Confirm {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        who: Option<String>,
        worktree: PathBuf,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice release` of a checkout or a preview, by its name.
    Release {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        name: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        accept_failure: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// `sluice branch create`, `accept` or `delete`.
    Branch {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        branch: crate::BranchCommand,
    },
    /// `sluice protect`, or `sluice protect --remove`.
    Protect {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        caller: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        remove: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        json: bool,
    },
    /// Serve `folders`, and the folders that the client adds after on the
    /// connection, each with its newest overview (design D6).
    Subscribe {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        folders: Vec<Added>,
    },
    /// The repository has work: hold it, and pass it at once.
    Serve { repository: PathBuf },
    /// One round of `branch` of the repository that holds `folder`,
    /// answered with the step's document (D10; the change
    /// `sluice-host-makes-each-change`, design D5).
    Step { folder: PathBuf, branch: String },
    /// A pass of the repository that holds `folder` now, answered once it
    /// ends (D14; design D5). `at`, RFC 3339, is the asker's time: where
    /// passes are taken only when asked, the pass is at that time, so the
    /// host process's time is its askers', however many ask.
    Pass {
        folder: PathBuf,
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
    /// Whether to let the git transaction `lines`, in git's `prepared`
    /// state, go: what branch protection's hook sends, answered with
    /// `output`, its refusal on standard error and its exit (the change
    /// `the-git-hook-asks-sluice-host`, design D1). `folder` is where git
    /// ran the hook, `git_dir` git's `GIT_DIR` there where it set one,
    /// and `by` the command line of the git that ran it.
    ReferenceTransaction {
        folder: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_dir: Option<PathBuf>,
        by: String,
        lines: String,
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
    /// A wait's step that ends the wait: the number of the repository's
    /// last pass whose report went to its followers once the passes in
    /// flight as it read had ended, which the wait says before it prints
    /// this (122, 123).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<u64>,
}

/// An answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Answer {
    /// What a session's command writes on standard output, as it writes
    /// it, before its `output` (the change
    /// `sluice-host-runs-a-sessions-checks`, design D1).
    Stdout(String),
    /// A line that a session's command says on standard error, as it says
    /// it, before its `output` (design D1).
    Stderr(String),
    /// A read's output (design D1).
    Output(Output),
    /// A wait's step (design D8).
    Waited(WaitStep),
    /// A submit's output, and the wait it follows next (the change
    /// `sluice-host-makes-each-change`, design D2).
    Submitted(Submitted),
    /// Done, for a serve or a hand-over.
    Ok,
    /// A step's document, as `sluice step --json` prints it.
    Step(serde_json::Value),
    /// A pass's report: for a pass, and each pass of a followed
    /// repository.
    Pass(Report),
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
                caller: BTreeMap::new(),
                target: Some("main".into()),
                all: false,
                json: true,
                branch: None,
            },
            Request::Overview {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
                json: false,
            },
            Request::Home {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
                worktree: "/w/feat".into(),
                json: true,
            },
            Request::Forecast {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
                branch: "main".into(),
                worktree: "/w/feat".into(),
                head: None,
                after: vec![4],
                json: true,
            },
            Request::Inbox {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
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
                caller: BTreeMap::new(),
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
            pass: None,
        });
        let line = serde_json::to_string(&output).unwrap();
        assert_eq!(
            line,
            r#"{"output":{"stdout":"{}\n","stderr":["sluice: warning: late"],"code":0}}"#
        );
        assert_eq!(serde_json::from_str::<Answer>(&line).unwrap(), output);
    }

    /// The change `the-git-hook-asks-sluice-host`, design D1: the hook's
    /// request names the transaction, the folder, `GIT_DIR` and git's
    /// command line, and reads back as it was sent.
    #[test]
    fn a_reference_transaction_reads_back() {
        let asked = Request::ReferenceTransaction {
            folder: "/w/feat".into(),
            git_dir: Some(".git".into()),
            by: "git update-ref refs/heads/main b a".into(),
            lines: "a b refs/heads/main\n".into(),
        };
        let mut line = Vec::new();
        send(&mut line, &asked).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&line).trim_end(),
            r#"{"ask":"reference-transaction","folder":"/w/feat","git_dir":".git","by":"git update-ref refs/heads/main b a","lines":"a b refs/heads/main\n"}"#
        );
        assert_eq!(receive::<Request>(&mut line.as_slice()).unwrap(), asked);
        let bare =
            br#"{"ask":"reference-transaction","folder":"/r.git","by":"git fetch","lines":""}"#;
        assert_eq!(
            serde_json::from_slice::<Request>(bare).unwrap(),
            Request::ReferenceTransaction {
                folder: "/r.git".into(),
                git_dir: None,
                by: "git fetch".into(),
                lines: String::new(),
            }
        );
    }

    /// The change `sluice-host-makes-each-change`, design D1, D2, D5: each
    /// change names its folder, its command's arguments and its caller,
    /// and reads back as it was sent; a step and a pass name a folder; a
    /// submit's answer and a pull request's step read back.
    #[test]
    fn the_changes_read_back() {
        let caller = BTreeMap::from([
            ("SLUICE_AGENT".to_string(), "feat".to_string()),
            (
                "TRACEPARENT".to_string(),
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
            ),
        ]);
        let changes = [
            Request::Submit {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                submit: SubmitAsk {
                    branch: "main".into(),
                    after: vec![3],
                    priority: 2,
                    no_wait: true,
                    subject: vec!["a line".into()],
                    worktree: vec![".".into(), "../b".into()],
                    json: true,
                    ..Default::default()
                },
            },
            Request::Amend {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                id: 4,
                after: Some(vec![2]),
                priority: Some(5),
                by: "planner".into(),
                json: true,
                branch: Some("main".into()),
            },
            Request::Withdraw {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                id: 4,
                by: "operator".into(),
                json: true,
                branch: None,
            },
            Request::Confirm {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                keys: vec!["4:landed".into()],
                who: Some("operator".into()),
                worktree: "/w/feat".into(),
                json: false,
            },
            Request::Release {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                name: "sluice/try/main/48".into(),
                accept_failure: true,
                json: true,
            },
            Request::Branch {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                branch: crate::BranchCommand::Create {
                    branch: "feature/x".into(),
                    from: "main".into(),
                    json: true,
                },
            },
            Request::Protect {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                remove: false,
                json: true,
            },
            Request::Previews {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
                json: true,
            },
            Request::Step {
                folder: "/w/feat".into(),
                branch: "main".into(),
            },
            Request::Pass {
                folder: "/w/feat".into(),
                at: None,
            },
        ];
        for change in changes {
            let mut line = Vec::new();
            send(&mut line, &change).unwrap();
            assert_eq!(receive::<Request>(&mut line.as_slice()).unwrap(), change);
        }
        let line = br#"{"ask":"withdraw","folder":"/w/app/feat","caller":{"SLUICE_AGENT":"feat"},"id":4,"by":"operator","json":true}"#;
        assert_eq!(
            serde_json::from_slice::<Request>(line).unwrap(),
            Request::Withdraw {
                folder: "/w/app/feat".into(),
                caller: BTreeMap::from([("SLUICE_AGENT".to_string(), "feat".to_string())]),
                id: 4,
                by: "operator".into(),
                json: true,
                branch: None,
            }
        );
        // D5: a step that names a repository and no folder is no request
        // of this release.
        assert!(serde_json::from_str::<Request>(
            r#"{"ask":"step","repository":"/w/app.git","branch":"main"}"#
        )
        .is_err());
        let submitted = Answer::Submitted(Submitted {
            output: Output {
                stdout: "submission 4 to main · nothing queued ahead\n".into(),
                stderr: vec![],
                code: 0,
                pass: None,
            },
            wait: Some(WaitAsk {
                target: "4".into(),
                own: Some(Followed {
                    id: 4,
                    branch: "main".into(),
                }),
                ..Default::default()
            }),
            queued: true,
        });
        let line = serde_json::to_string(&submitted).unwrap();
        assert_eq!(serde_json::from_str::<Answer>(&line).unwrap(), submitted);
        let step = WaitStep::Pull {
            pull: Box::new(PullWait {
                branch: "main".into(),
                number: 7,
                delivered_to: Some("/w/feat".into()),
                takes_own: true,
                last: Some(("waiting: review".into(), vec!["needs 1 review".into()])),
                ..Default::default()
            }),
            said: "waiting: review\n".into(),
            stderr: vec![],
        };
        let line = serde_json::to_string(&step).unwrap();
        assert_eq!(serde_json::from_str::<WaitStep>(&line).unwrap(), step);
        // An act's pull requests, one decided with its names, read back
        // inside a wait's request, whose fields serde reads flattened.
        let sub: sluice_core::records::Submission = serde_json::from_value(serde_json::json!({
            "schema": "sluice/submission@1",
            "id": 1, "branch": "main", "head": "5741393bd46752f9f125b25587c709243b0a0c10",
            "from": {"agent": "unit/w1", "worktree": "pull/1"},
            "submitted": "2026-11-23T17:49:36Z", "after": [], "priority": 0,
            "attempt": 1, "state": "landed",
        }))
        .unwrap();
        let wait = Request::Wait {
            folder: "/w/feat".into(),
            caller: BTreeMap::new(),
            wait: WaitAsk {
                target: "main".into(),
                pulls: Some(Box::new(PullGroupWait {
                    branch: "main".into(),
                    told: None,
                    items: vec![PullItem {
                        number: 1,
                        worktree: "/w/a".into(),
                        copy: None,
                        id: Some(1),
                        ahead: vec![],
                        decided: Some(crate::Followed::Done(
                            sub,
                            std::collections::BTreeMap::from([(1, "#1".to_string())]),
                        )),
                        last: None,
                        there: None,
                        said_there: None,
                    }],
                })),
                ..Default::default()
            },
        };
        let line = serde_json::to_string(&wait).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), wait);
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

    /// The change `sluice-host-runs-a-sessions-checks`, design D1, D2:
    /// `exec`, `preview`, `equip` and a worktree's `release` each name the
    /// folder, the command's arguments, the caller and the caller's run
    /// variables, and read back as they were sent; the streamed `stdout`
    /// and `stderr` answers read back.
    #[test]
    fn a_sessions_commands_and_their_streamed_answers_read_back() {
        let caller = BTreeMap::from([("SLUICE_AGENT".to_string(), "a".to_string())]);
        let run_env = BTreeMap::from([("PATH".to_string(), "/b:/usr/bin".to_string())]);
        let asks = [
            Request::Exec {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                run_env: run_env.clone(),
                otel: Default::default(),
                branch: "main".into(),
                command: vec!["sh".into(), "-c".into(), "echo one".into()],
                worktree: "/w/feat".into(),
                json: false,
                release: true,
            },
            Request::Preview {
                folder: "/w/feat".into(),
                caller: caller.clone(),
                run_env: run_env.clone(),
                otel: Default::default(),
                branch: "main".into(),
                revision: "2eb676c".into(),
                wait: true,
                json: true,
            },
            Request::Equip {
                folder: "/w/feat/src".into(),
                caller: caller.clone(),
                run_env: run_env.clone(),
                otel: Default::default(),
                branch: "main".into(),
                at: Some("2eb676c".into()),
                json: false,
            },
            Request::ReleaseWorktree {
                folder: "/w/feat".into(),
                caller,
                run_env,
                otel: crate::telemetry::Choices {
                    otel: Some(true),
                    ..Default::default()
                },
                worktree: "/w/feat".into(),
                accept_failure: true,
                json: true,
            },
        ];
        for ask in asks {
            let mut line = Vec::new();
            send(&mut line, &ask).unwrap();
            assert_eq!(receive::<Request>(&mut line.as_slice()).unwrap(), ask);
        }
        let line = br#"{"ask":"exec","folder":"/w/feat","branch":"main","worktree":"/w/feat"}"#;
        assert_eq!(
            serde_json::from_slice::<Request>(line).unwrap(),
            Request::Exec {
                folder: "/w/feat".into(),
                caller: BTreeMap::new(),
                run_env: BTreeMap::new(),
                otel: Default::default(),
                branch: "main".into(),
                command: vec![],
                worktree: "/w/feat".into(),
                json: false,
                release: false,
            }
        );
        for (answer, line) in [
            (
                Answer::Stdout("running cli:test ...\n".into()),
                r#"{"stdout":"running cli:test ...\n"}"#,
            ),
            (
                Answer::Stderr("waiting: the host is at its bound".into()),
                r#"{"stderr":"waiting: the host is at its bound"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&answer).unwrap(), line);
            assert_eq!(serde_json::from_str::<Answer>(line).unwrap(), answer);
        }
    }

    /// Design D5: `hold` is no request of this release.
    #[test]
    fn a_hold_is_no_request() {
        let older = br#"{"ask":"hold","kind":"session","repository":"/host/repo.git","place":"unit/a","branch":"main"}"#;
        assert!(serde_json::from_slice::<Request>(older).is_err());
    }
}
