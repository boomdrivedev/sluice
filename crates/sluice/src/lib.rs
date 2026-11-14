//! The `sluice` command: its arguments, what it says, and its exits
//! (walkthrough sections 5, 6 and 9).
//!
//! Every command runs against [`Seams`] it is given, so the same code
//! runs over the fakes in tests and over the real profiles that
//! [`profiles`] binds.

pub mod branch;
pub mod equip;
pub mod exec;
pub mod hand_over;
pub mod home;
pub mod host_count;
pub mod host_process;
pub mod host_settings;
pub mod identity;
pub mod instance;
pub mod overview;
pub mod preview;
pub mod profiles;
pub mod progress;
pub mod protect;
pub mod reader;
pub mod retry;
pub mod runs;
pub mod scrub;
pub mod sessions;
pub mod signing;
pub mod telemetry;
pub mod told;
pub mod words;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use overview::Follows;
use reader::{Driver, Drives, Reader};
use sluice_core::overview::{in_queue, BranchView, CandidateView, Held};
use sluice_core::records::{Beside, BranchHold, CopyBehind, Event, FailedTask, State, Submission};
use sluice_core::{
    Accepted, AmendRefusal, Clock, Collect, CopyDiffers, Forecast, Forecasted, GroupRequest,
    Lookup, Owed, Requester, Seams, Several, Sluice, Status, Step, SubmitHead, SubmitRefusal,
    SubmitRequest, Unread, WithdrawRefusal, Withdrew,
};
use sluice_seams::{Decided, Outcome, Rev, SessionPlace, Submitter};
use told::Kind;

/// Exits of a waiting submit and `sluice wait` (walkthrough section 5;
/// ADR 0041).
pub const LANDED: i32 = 0;
pub const FAILED: i32 = 1;
pub const CONFLICT: i32 = 2;
/// sluice itself failed, or a wait could not read: the cause is on
/// stderr, and it is no outcome.
pub const INTERNAL: i32 = 3;
/// The submission was withdrawn while it was waited on.
pub const WITHDRAWN: i32 = 4;
/// `sluice wait --timeout` ran out with the submission undecided.
pub const WAITING: i32 = 5;
/// How long a waiting submit or `sluice wait` gives each call to a remote,
/// tried again under the retry policy through a forge's 5xx answers and
/// whatever else the remote never judged, before it says it could not read
/// ([`INTERNAL`]) (134); what it could not read decides nothing.
pub const UNANSWERED_FOR: chrono::TimeDelta = chrono::TimeDelta::minutes(5);
/// A command line sluice cannot read: an unknown command, flag or value.
/// Kept apart from [`CONFLICT`], so a mistyped flag is never read as a
/// conflict.
pub const USAGE: i32 = 64;

/// sluice's version: the workspace's in `Cargo.toml`, as the tree it was
/// built from gives it. main gives 0.0.0, which no release has; a release's
/// tag names a release commit in which `set-version.sh` wrote the release's,
/// so a build of the tag says it with nothing given (record 0111; core.55).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The root's help: examples, and the telemetry flags every command takes,
/// listed here alone so they do not repeat in each command's help.
const ROOT_AFTER_HELP: &str = "\
Examples:
  $ sluice                  where this worktree stands
  $ sluice submit main      land this worktree's head on main
  $ sluice overview         every branch's queue

Telemetry, taken by every command:
      --otel                       Export traces, metrics and logs over
                                   OTLP/HTTP, where the host's [telemetry] in
                                   ~/.config/sluice/config.toml says; this flag
                                   and SLUICE_OTEL choose for this command alone,
                                   --otel=false or SLUICE_OTEL=0 turning the
                                   host's send = true off. OTEL_SDK_DISABLED=true
                                   turns them all off, and OTEL_TRACES_EXPORTER,
                                   OTEL_METRICS_EXPORTER or OTEL_LOGS_EXPORTER at
                                   none turns one alone off
                                   [env: SLUICE_OTEL=1|true|yes|on|0|false|no|off]
      --otel-service-name <NAME>   The service the telemetry is reported as
                                   [env: SLUICE_OTEL_SERVICE_NAME]
                                   [default: the host's [telemetry], else sluice]
      --otel-logs <LOGS>           How much of a failed check's output is sent as
                                   logs: off, summary (where it is kept, never
                                   what it says) or tail (its end, scrubbed of
                                   what reads as a secret) [env: SLUICE_OTEL_LOGS]
                                   [default: the host's [telemetry], else summary]
      --otel-logs-limit <BYTES>    The most of a failed check's output sent at
                                   tail [env: SLUICE_OTEL_LOGS_LIMIT]
                                   [default: the host's [telemetry], else 8192]";

/// `sluice submit --help`'s examples and exits; the exits are those of
/// the reference's "Exit codes".
const SUBMIT_AFTER_HELP: &str = "\
Examples:
  $ sluice submit main
  $ sluice submit main --no-wait
  $ sluice submit main --no-wait --subject \"Batch review cards\"   its pull request's title
  $ sluice submit main --after 12,14 --priority 5
  $ sluice submit main --worktree ../b1 --worktree ../b2   both heads as one act
  $ sluice submit main --head feature/x --no-wait   a branch sluice lands on, by name

Exit codes:
  0    landed, whatever failed after the landing (said on stderr)
  1    failed: this submission was blamed, or its pull request closed
  2    a conflict, or a refusal
  3    sluice itself failed, or it could not read the forge for 5m and
       whether it landed is not known; the cause is on stderr
  4    withdrawn while it waited
  64   a command line sluice cannot read
  130  stopped with Ctrl-C; the submission stays queued

Several worktrees are submitted all or none; the exit is 0 when every one
landed, and otherwise the exit of the first, in the order given, that did not.";

/// `sluice wait --help`'s examples and exits (ADR 0041).
const WAIT_AFTER_HELP: &str = "\
Examples:
  $ sluice wait 12
  $ sluice wait 12 --timeout 9m --json   an agent's loop: again while it exits 5
  $ sluice wait main                     this worktree's last submission to main

Exit codes:
  0    landed, whatever failed after the landing (said on stderr)
  1    failed: it was blamed, or its pull request closed; or no such
       submission on any branch, every branch read
  2    a conflict
  3    sluice itself failed, or it could not read the forge for 5m and
       whether it landed is not known: the cause is on stderr; or the
       number is on none of the branches it could read, and each branch
       it could not read is named with why
  4    withdrawn
  5    still waiting: --timeout ran out, and the submission stays queued
  64   a command line sluice cannot read; or the number is on more than
       one branch and no --branch names one: each branch is named
  130  stopped with Ctrl-C; the submission stays queued";

/// `sluice inbox --help`'s examples and exits (51, 135).
const INBOX_AFTER_HELP: &str = "\
Examples:
  $ sluice inbox                    once, when a session starts again
  $ sluice inbox --json
  $ sluice inbox --confirm --json   a caller that keeps what it is given
  $ sluice inbox --new              only what nothing has handed over yet

Each outcome is handed over again by every inbox until it is confirmed:
sluice confirm <key> once it has been acted on.

Exit codes:
  0    handed over every outcome owed, or there was none
  3    a branch's records could not be read, said on stderr after the rest
       is handed over; or sluice itself failed
  64   a command line sluice cannot read";

/// `sluice confirm --help`'s examples and exits (51).
const CONFIRM_AFTER_HELP: &str = "\
Examples:
  $ sluice confirm 12:landed
  $ sluice confirm 12:landed 15:conflict
  $ sluice confirm 13:failed --as operator   a removed worktree's

Exit codes:
  0    confirmed, or confirmed before
  1    no outcome is owed under a key
  2    a key's outcome is owed to another worktree: --as operator confirms it
  3    a branch's records could not be read, so a key not found may be
       there; or sluice itself failed
  64   a command line sluice cannot read";

/// `--timeout`'s value, read as the policy reads a duration; one it cannot
/// read is a command line sluice cannot read (64). `wt submit`'s own
/// `--timeout` is read by it too.
pub fn timeout(text: &str) -> std::result::Result<std::time::Duration, String> {
    sluice_core::policy::parse_duration(text).map_err(|e| format!("{e:#}"))
}

#[derive(Parser, Debug)]
#[command(
    name = "sluice",
    version = VERSION,
    about = "A merge queue for agents in worktrees.",
    long_about = "A merge queue for agents in worktrees.\n\n\
                  With no command, where this worktree stands: its submission, the \
                  branches' queues, its ports and what to run next.",
    after_help = ROOT_AFTER_HELP,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// None: the home view, where this worktree stands.
    #[command(subcommand)]
    pub command: Option<Command>,
    /// With no command: the home view as one JSON document.
    #[arg(long)]
    pub json: bool,
    /// With no command: the worktree to show.
    #[arg(long, default_value = ".")]
    pub worktree: String,
    // The telemetry flags are hidden, here and in every command, and
    // listed in ROOT_AFTER_HELP instead.
    /// Export traces, metrics and logs over OTLP/HTTP, where the host's
    /// `[telemetry]` in `~/.config/sluice/config.toml` says (151). This
    /// flag, or SLUICE_OTEL, chooses for this command alone:
    /// `--otel=false` or SLUICE_OTEL=0 turns the host's `send = true` off.
    /// OTEL_SDK_DISABLED=true turns them all off, and OTEL_TRACES_EXPORTER,
    /// OTEL_METRICS_EXPORTER or OTEL_LOGS_EXPORTER at `none` turns that one
    /// alone off. SLUICE_OTEL takes `1`, `true`, `yes` or `on`, and `0`,
    /// `false`, `no` or `off`.
    #[arg(
        long,
        global = true,
        hide = true,
        env = "SLUICE_OTEL",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub otel: Option<bool>,
    /// The service the telemetry is reported as; where unset, the host's
    /// `[telemetry] service`, else `sluice`.
    #[arg(long, global = true, hide = true, env = "SLUICE_OTEL_SERVICE_NAME")]
    pub otel_service_name: Option<String>,
    /// While telemetry is on, how much of a failed check's output is sent
    /// as logs: `off`, `summary` (where it is kept, never what it says) or
    /// `tail` (its end, scrubbed of what reads as a secret); where unset,
    /// the host's `[telemetry] logs`, else `summary`.
    #[arg(long, global = true, hide = true, env = "SLUICE_OTEL_LOGS", value_enum)]
    pub otel_logs: Option<telemetry::Logs>,
    /// The most of a failed check's output sent at `tail`, in bytes, held
    /// to 65536; where unset, the host's `[telemetry] logs_tail_bytes`,
    /// else 8192.
    #[arg(long, global = true, hide = true, env = "SLUICE_OTEL_LOGS_LIMIT")]
    pub otel_logs_limit: Option<usize>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Submit this worktree's head to a branch, and wait for the outcome.
    #[command(after_help = SUBMIT_AFTER_HELP)]
    Submit {
        /// The branch to land on.
        branch: String,
        /// Submissions this one lands after.
        #[arg(long, value_delimiter = ',')]
        after: Vec<u64>,
        /// Higher lands sooner; equal ones go in the order submitted.
        #[arg(long, default_value_t = 0)]
        priority: i64,
        /// Return once submitted.
        #[arg(long)]
        no_wait: bool,
        /// One line saying what lands: the title of the pull request this
        /// submit opens, or of one it opened before, and the name of its
        /// landing; with several worktrees, once for each, in their order.
        #[arg(long, value_name = "TEXT", conflicts_with_all = ["check", "status"])]
        subject: Vec<String>,
        /// Only say whether it would conflict; submit nothing.
        #[arg(long)]
        check: bool,
        /// The branch to submit in place of this worktree's head, a branch
        /// sluice lands on, from anywhere in the repository; with --check,
        /// the branch whose revision to forecast.
        #[arg(long, value_name = "BRANCH")]
        head: Option<String>,
        /// Only say where this worktree's last submission, or its pull
        /// request, stands.
        #[arg(long)]
        status: bool,
        /// The worktree to submit from, this folder where none is given;
        /// given more than once, every one's head as one act, in the order
        /// given.
        #[arg(long)]
        worktree: Vec<String>,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
        /// Waiting, confirm the outcome it reports on receipt, for a
        /// caller that keeps what it is given (51).
        #[arg(long, conflicts_with_all = ["no_wait", "check", "status"])]
        confirm: bool,
        /// Waiting, confirm on receipt a landing alone: `wt submit`'s, for
        /// a landing whose worktree it removes.
        #[arg(long, hide = true)]
        confirm_landing: bool,
    },
    /// Wait for a submission's outcome, running sluice's rounds meanwhile
    /// unless `sluice run` drives the repository on this host, as a waiting
    /// submit does.
    #[command(after_help = WAIT_AFTER_HELP)]
    Wait {
        /// A submission's number, or a branch: this worktree's last
        /// submission to it.
        target: String,
        /// Return after this long, as `90s`, `9m` or `1h30m`, with the
        /// submission as it stands.
        #[arg(long, value_parser = timeout)]
        timeout: Option<std::time::Duration>,
        /// The worktree whose last submission a branch names.
        #[arg(long, default_value = ".")]
        worktree: String,
        /// One JSON document on stdout: the submission's record.
        #[arg(long)]
        json: bool,
        /// Confirm the outcome it reports on receipt, for a caller that
        /// keeps what it is given (51).
        #[arg(long)]
        confirm: bool,
        /// Confirm on receipt a landing alone: `wt submit`'s, for a
        /// landing whose worktree it removes.
        #[arg(long, hide = true)]
        confirm_landing: bool,
        /// The branch the submission's number is on, where more than one
        /// branch holds it.
        #[arg(long)]
        branch: Option<String>,
    },
    /// Hand over every outcome still owed to this worktree's submissions,
    /// printing each with the key to confirm it by; each is handed over
    /// again until it is confirmed.
    #[command(after_help = INBOX_AFTER_HELP)]
    Inbox {
        /// The worktree whose outcomes are collected.
        #[arg(long, default_value = ".")]
        worktree: String,
        /// One JSON document on stdout: `{"outcomes":[...]}`, each the
        /// submission's record with its `delivery`.
        #[arg(long)]
        json: bool,
        /// Confirm each outcome on receipt, for a caller that keeps what it
        /// is given for readers of its own.
        #[arg(long)]
        confirm: bool,
        /// Only the outcomes no way has handed over yet: no notice pushed,
        /// no inbox or wait that printed them.
        #[arg(long)]
        new: bool,
    },
    /// Confirm an outcome once it has been acted on, by the key its notice
    /// and `sluice inbox` give it, so it is handed over no more.
    #[command(after_help = CONFIRM_AFTER_HELP)]
    Confirm {
        /// Each outcome's key, as `12:landed`.
        #[arg(required = true)]
        keys: Vec<String>,
        /// `operator`: confirm for the outcome's own reader, such as a
        /// worktree removed before it confirmed.
        #[arg(long = "as", value_parser = ["operator"])]
        who: Option<String>,
        /// The worktree whose outcomes are confirmed.
        #[arg(long, default_value = ".")]
        worktree: String,
        /// One JSON document on stdout: `{"confirmed":[...]}`.
        #[arg(long)]
        json: bool,
    },
    /// One round of a branch's work, then exit.
    #[command(after_help = "Example:\n  $ sluice step main")]
    Step {
        /// The branch to run a round of.
        branch: String,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Rounds for every branch with a queue, until stopped.
    #[command(after_help = "Examples:\n  $ sluice run\n  $ sluice run --rounds 1")]
    Run {
        /// Stop after this many rounds; a run whose last round passed a
        /// branch by exits 3, naming it.
        #[arg(long)]
        rounds: Option<usize>,
    },
    /// A branch's queue, or one submission; with neither, every worktree
    /// sluice holds ports or runs for.
    #[command(after_help = "Examples:\n  $ sluice status main --all\n  $ sluice status 42 --json")]
    Status {
        /// A branch, or a submission's number.
        target: Option<String>,
        /// With a branch, every submission it has had, not only those
        /// still in the queue, as text or `--json`.
        #[arg(long)]
        all: bool,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
        /// The branch the submission's number is on, where more than one
        /// branch holds it.
        #[arg(long)]
        branch: Option<String>,
    },
    /// Every branch's queue at a glance: its combinations under test, its
    /// submissions, its latest landings and blames, and the worktrees and
    /// previews held.
    #[command(after_help = "Examples:\n  $ sluice overview\n  $ sluice overview --json --watch")]
    Overview {
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
        /// Print the document again, as one line, each time it changes.
        #[arg(long, requires = "json")]
        watch: bool,
    },
    /// Change a queued submission's order, until it is under test.
    #[command(
        after_help = "Examples:\n  $ sluice amend 42 --after 40\n  $ sluice amend 42 --priority 5"
    )]
    Amend {
        /// The submission's number.
        id: u64,
        /// Submissions this one lands after, in place of those it had.
        #[arg(long, value_delimiter = ',')]
        after: Option<Vec<u64>>,
        /// Its new priority: higher lands sooner.
        #[arg(long)]
        priority: Option<i64>,
        /// Who asks: `planner`, `operator`, or a session's agent.
        #[arg(long = "as", default_value = "planner")]
        by: String,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
        /// The branch the submission's number is on, where more than one
        /// branch holds it.
        #[arg(long)]
        branch: Option<String>,
    },
    /// Withdraw a submission.
    #[command(after_help = "Example:\n  $ sluice withdraw 42")]
    Withdraw {
        /// The submission's number.
        id: u64,
        /// Who asks: `planner`, `operator`, or a session's agent.
        #[arg(long = "as", default_value = "operator")]
        by: String,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
        /// The branch the submission's number is on, where more than one
        /// branch holds it.
        #[arg(long)]
        branch: Option<String>,
    },
    /// Set aside a recorded result its host caused, not its test: no gate
    /// reads it again, and its line stays. The operator's to do, never a
    /// session's.
    #[command(
        name = "set-aside",
        after_help = "Example:\n  $ sluice set-aside cli:tools --at 2026-10-17T13:05:32Z \\\n      --reason \"OrbStack was down\""
    )]
    SetAside {
        /// The test, as its result names it.
        task: String,
        /// When the result was recorded, as `sluice status` gives it.
        #[arg(long)]
        at: String,
        /// The start of the result's key, when two were recorded at once.
        #[arg(long)]
        hash: Option<String>,
        /// Why the result says nothing of the test.
        #[arg(long)]
        reason: String,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Run the checks this worktree's head affects against a branch, or a
    /// command after `--`, under the host's bound, the worktree's ports
    /// and the branch's limits.
    #[command(after_help = "Examples:\n  $ sluice exec main\n  $ sluice exec main -- cargo test")]
    Exec {
        /// The branch whose policy the run is held to.
        branch: String,
        /// The command to run instead of the checks.
        #[arg(last = true)]
        command: Vec<String>,
        /// The worktree to run in.
        #[arg(long, default_value = ".")]
        worktree: String,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
        /// Release the worktree once the run ends.
        #[arg(long)]
        release: bool,
    },
    /// Equip this worktree for a branch's runner, as sluice equips every
    /// checkout before it runs anything there; it holds nothing.
    #[command(
        long_about = "Equip this worktree for a branch's runner, as sluice equips every \
                      checkout before it runs anything there: the toolchain the branch's \
                      policy names, then the runner's own steps, through its setup. It \
                      holds nothing: no ports, no record, no part of the host's bound.",
        after_help = "Examples:\n  $ sluice equip main\n  $ sluice equip main --at 2eb676c --json\n\n\
                      Exit codes:\n  0    every step passed, or there was none\n  \
                      1    a step failed\n  2    a refusal\n  3    sluice itself failed\n  \
                      64   a command line sluice cannot read"
    )]
    Equip {
        /// The branch whose policy equips it.
        branch: String,
        /// The revision whose policy equips it, whether or not it names the
        /// branch; the branch's current revision where none is given.
        #[arg(long)]
        at: Option<String>,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Bring up a running copy of a revision a branch has landed, at
    /// addresses a person opens; held until `sluice release <preview>`.
    #[command(
        after_help = "Examples:\n  $ sluice preview main 2eb676c\n  $ sluice preview main 2eb676c --wait --json"
    )]
    Preview {
        /// The branch that landed it.
        branch: String,
        /// The revision.
        revision: String,
        /// Return once its command has ended, exiting 1 if it failed.
        #[arg(long)]
        wait: bool,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Every preview on this host, with its addresses and state.
    #[command(after_help = "Examples:\n  $ sluice previews\n  $ sluice previews --json")]
    Previews {
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Release this worktree: run the policy's release command, end what
    /// its runs left running, free its ports. The worktree stays. Given a
    /// preview, or a checkout held because its release failed, release
    /// that.
    #[command(
        after_help = "Examples:\n  $ sluice release\n  $ sluice release 2eb676c\n  $ sluice release sluice/try/main/48 --accept-failure"
    )]
    Release {
        /// A preview, by its name or its revision, or a held checkout, by
        /// its name.
        name: Option<String>,
        /// The worktree to release.
        #[arg(long, default_value = ".")]
        worktree: String,
        /// Free what sluice holds for it without running the release
        /// command: its runs ended, the checkout sluice made removed, its
        /// ports let go.
        #[arg(long)]
        accept_failure: bool,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Run one check run's command to its end; started by a step.
    #[command(hide = true)]
    Supervise { dir: std::path::PathBuf },
    /// Install the hook that refuses updates to protected branches by
    /// anything but sluice; `submit`, `step` and `run` install it too.
    #[command(after_help = "Examples:\n  $ sluice protect\n  $ sluice protect --remove")]
    Protect {
        /// Take it out instead.
        #[arg(long)]
        remove: bool,
        /// One JSON document on stdout; the rest on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Run by git's hook: `reference-transaction <state>`, updates on stdin.
    #[command(hide = true)]
    Hook { name: String, state: String },
    // branches-1 104, 105; 121.
    /// Make or remove a branch sluice lands on, on its remote when its
    /// policy names one, or accept the revision a held branch is read at.
    #[command(
        after_help = "Examples:\n  $ sluice branch create feature/x --from main\n  $ sluice branch delete feature/x --into main\n  $ sluice branch accept feature/x 10e4791"
    )]
    Branch {
        #[command(subcommand)]
        command: BranchCommand,
    },
    /// sluice's identity on a forge.
    #[command(after_help = "Example:\n  $ sluice forge check main")]
    Forge {
        #[command(subcommand)]
        command: ForgeCommand,
    },
    /// Run by sluice's own git calls as their credential helper:
    /// `credential get`, git's credential protocol on stdin.
    #[command(hide = true)]
    Credential { operation: String },
}

/// Who pushes a branch that lives on a remote (C.4 pushed refs), for
/// `sluice status`.
pub struct Pusher {
    pub remote: String,
    /// sluice's own identity there; `None` for the host's credentials.
    pub identity: Option<String>,
}

pub trait Pushers {
    /// `None` for a branch that lives on no remote.
    fn pusher(&self, branch: &str) -> Result<Option<Pusher>>;
}

/// Every branch local.
impl Pushers for () {
    fn pusher(&self, _: &str) -> Result<Option<Pusher>> {
        Ok(None)
    }
}

/// What `sluice submit` to a branch with pull requests opened or updated.
pub struct Opened {
    pub number: u64,
    /// What it did: the branch pushed, if it pushed one, and the pull
    /// request.
    pub said: String,
    /// 124: the worktree's head, this host's copy of a branch sluice
    /// lands on, where it differs from the remote's revision submitted.
    pub copy: Option<CopyDiffers>,
    /// 125: the submission a group submit queued for it, having taken its
    /// group whole; `None` for one submit's, which intake takes.
    pub submission: Option<Accepted>,
}

/// 125: a pull request of a group submit short of its conditions.
pub struct Short {
    /// Its worktree, as the act named it.
    pub worktree: String,
    pub number: u64,
    /// The reviews and checks it lacks on its head, as a waiting pull
    /// request names them.
    pub lacks: Vec<String>,
}

/// 125: why a group submit to a branch with pull requests queued nothing.
pub enum GroupRefused {
    /// Worktrees refused before anything was pushed, each with why.
    Worktrees(Vec<(String, SubmitRefusal)>),
    /// Every branch pushed and its pull request opened or updated, as
    /// `opened` says, and each left a draft at its head: `short` fall
    /// short of their conditions.
    Short {
        opened: Vec<Opened>,
        short: Vec<Short>,
    },
}

/// 137, C.5: a pull request's queue on the host that posts sluice's check
/// on its head, as that check carries it for any host to read: the
/// posting host; whether its records hold a submission of the request at
/// that head, that submission's number and state as its record names it,
/// `None` while not taken in; the submissions ahead of it there and not
/// out of the queue, in queue order; and what a waiting request waits
/// for, in the words its check's title gives.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Queue {
    pub host: String,
    pub taken_in: bool,
    pub submission: Option<u64>,
    pub state: Option<String>,
    pub ahead: Vec<Ahead>,
    pub waits_for: Vec<String>,
}

/// One submission ahead of a pull request's in its queue: its number on
/// the posting host, and its pull request's number, `None` for a
/// worktree's.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ahead {
    pub submission: u64,
    pub pull_request: Option<u64>,
}

/// 137: which process of its host holds a branch's intake mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Holder {
    /// The host's `sluice run`, the holder of its driver's lock.
    Driver,
    /// A `sluice wait` or a waiting submit.
    Wait,
}

/// 137, design D2: the host a branch's intake mark names, as this host's
/// last fetch brought it, and whether that host's `sluice run` or a wait
/// holds it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TakenInBy {
    pub host: String,
    pub holder: Holder,
}

impl TakenInBy {
    /// Its holder in words: `b's sluice run`, or `a wait on b`.
    pub fn said(&self) -> String {
        match self.holder {
            Holder::Driver => format!("{}'s sluice run", self.host),
            Holder::Wait => format!("a wait on {}", self.host),
        }
    }
}

/// 137, design D5: the pull requests into a branch this host follows and
/// its queue does not hold at their heads, each as the host that takes
/// them in shows it, for `sluice status <branch>`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Across {
    /// The host the branch's intake mark names and its holder, as this
    /// host's last fetch brought it; `None` where no mark stands.
    pub taken_in_by: Option<TakenInBy>,
    pub pull_requests: Vec<AcrossRow>,
}

/// 137, design D5: one pull request a host follows, as last read.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AcrossRow {
    pub pull_request: u64,
    /// Its title, which a person knows it by (26).
    pub title: String,
    /// The branch it is opened from.
    pub from: String,
    pub head: String,
    pub draft: bool,
    /// Where it stands, as a wait across hosts prints it: sluice's check's
    /// title on its head, `waiting` where none stands; `landed` or `closed
    /// without landing` once a read found it so, said once.
    pub state: String,
    /// The queue sluice's check on its head carries; `None` where the
    /// check carries none, or none stands.
    pub queue: Option<Queue>,
    /// When a process of the host last read it from GitHub; `None` where
    /// none has.
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The state of an [`AcrossRow`] found merged.
pub const ACROSS_LANDED: &str = "landed";
/// The state of an [`AcrossRow`] found closed and not merged.
pub const ACROSS_CLOSED: &str = "closed without landing";

impl Across {
    /// Design D5: the heading of the rows in words: the host that takes
    /// `branch`'s pull requests in and its holder, or that no host holds
    /// the mark.
    pub fn heading(&self, branch: &str) -> String {
        match &self.taken_in_by {
            Some(by) if by.holder == Holder::Driver => {
                format!("pull requests {} takes in, by its sluice run:", by.host)
            }
            Some(by) => format!("pull requests {} takes in, by {}:", by.host, by.said()),
            None => {
                format!("pull requests this host follows; no host holds {branch}'s intake mark:")
            }
        }
    }

    /// Design D2, D5: one row in words: its number and title (26), then
    /// the submission the check names on the host that posted it and
    /// where it stands there, or that it waits to be taken in by the mark's
    /// host and on what.
    pub fn line(&self, row: &AcrossRow) -> String {
        let mark = self.taken_in_by.as_ref().map(|by| by.host.as_str());
        let held = row
            .queue
            .as_ref()
            .filter(|q| q.taken_in)
            .and_then(|q| Some((q.submission?, q.host.as_str())));
        let at = match held {
            _ if row.state == ACROSS_LANDED || row.state == ACROSS_CLOSED => row.state.clone(),
            Some((id, host)) if Some(host) == mark => {
                format!("submission {id} there · {}", row.state)
            }
            Some((id, host)) => format!("submission {id} on {host} · {}", row.state),
            None => {
                let by = mark.map_or(String::new(), |h| format!(" by {h}"));
                match row.state.strip_prefix("waiting: ") {
                    Some(on) => format!("waiting to be taken in{by}: {on}"),
                    None => format!("waiting to be taken in{by}"),
                }
            }
        };
        let draft = if row.draft { " (a draft)" } else { "" };
        format!("#{} {}{draft} · {at}", row.pull_request, row.title)
    }
}

/// Where a pull request stands, for a submit that follows it.
pub enum Followed {
    /// Not decided yet: its state, and lines beneath it.
    Open {
        title: String,
        lines: Vec<String>,
        /// 137, design D2: where another host takes its branch's pull
        /// requests in, the host the intake mark names and its holder, as
        /// this host's last fetch brought it; `None` where no mark stands,
        /// and on the host that takes them in.
        taken_in_by: Option<TakenInBy>,
        /// 137, design D2: where another host takes its branch's pull
        /// requests in, the queue sluice's check on its current head
        /// carries; `None` where the check carries none, and on the host
        /// that takes them in.
        queue: Option<Queue>,
        /// The submission intake took for it on its current head,
        /// undecided; `None` while it waits to be taken in (C.5 pull
        /// request, 120).
        submission: Option<Submission>,
        /// What each submission is called on a pull request, as
        /// [`Followed::Done`] names them.
        names: BTreeMap<u64, String>,
        /// The revision the forge holds as its head; `None` when the forge
        /// returned no pull to read it from.
        head: Option<String>,
        /// Whether it is a draft, which intake does not take in.
        draft: bool,
    },
    /// Its submission on its head landed, failed or conflicts, with what
    /// each submission is called on a pull request.
    Done(Submission, BTreeMap<u64, String>),
    /// 137: merged, while another host takes its branch's pull requests
    /// in: sluice's check's title and summary on its head, where it says
    /// it landed, and the head and draft mark the forge held for it, which
    /// `--status` answers with (124; record 0096).
    Landed {
        title: String,
        summary: String,
        head: String,
        draft: bool,
    },
    /// 137: sent back on its head, while another host takes its branch's
    /// pull requests in: failed, or in conflict, as sluice's check there
    /// names it, and the head and draft mark the forge holds for it, which
    /// `--status` answers with (124; record 0096).
    SentBack {
        conflict: bool,
        title: String,
        summary: String,
        head: String,
        draft: bool,
    },
    /// Closed without landing.
    Closed,
}

/// 137: which process takes a branch's pull requests in, for its intake
/// mark: `sluice run`, the host's driver, which pushes the mark when it
/// starts; a wait on a host where no driver lives, which pushes it where
/// none stands and lets it go when it ends; or a step, a submit that does
/// not wait, or a wait's first pass beside a live driver, which pushes
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taker {
    Driver,
    Wait,
    Step,
}

/// 137: lets go of the intake marks a wait holds when the wait ends, at
/// every exit, Ctrl-C's among them.
struct LetGo<'a>(Option<&'a dyn PullRequests>);

impl Drop for LetGo<'_> {
    fn drop(&mut self) {
        if let Some(pulls) = self.0 {
            pulls.let_go();
        }
    }
}

/// A branch whose work arrives as pull requests on its remote (C.5 pull
/// request, C.7 `pull_requests`): the driver that takes them into its
/// queue and opens them for `submit`.
pub trait PullRequests {
    /// The branches `sluice run` serves for their pull requests, beside
    /// those with records: each the policy on the default branch's
    /// revision turns pull requests on for.
    fn branches(&self, seams: Seams<'_>) -> Result<Vec<String>>;

    /// Before a round of `branch`: submit each pull request ready, withdraw
    /// each changed, and post each one's state, where this host takes the
    /// branch's pull requests in as `taker` (137). `false` for a branch
    /// that takes no pull requests.
    fn intake(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
        taker: Taker,
    ) -> Result<bool>;

    /// 137: the host that takes `branch`'s pull requests in, as this
    /// process's last intake found it, where that is another host.
    fn elsewhere(&self, _branch: &str) -> Option<String> {
        None
    }

    /// 137: let go of the intake marks this process holds as a wait.
    fn let_go(&self) {}

    /// Open the pull request from `head`'s branch into `branch`, or update
    /// the one open, and mark it ready; `subject`, the submitter's line
    /// saying what lands, titles it, and the branch where none is given.
    /// A worktree's branch sluice lands on nowhere is pushed first; a
    /// branch sluice lands on, a worktree's or one named (124), is asked
    /// for as its remote holds it, and nothing is pushed. A branch named
    /// is read through the core's session, never the driver's (C.5; record
    /// 0090).
    fn open(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        head: &SubmitHead,
        branch: &str,
        subject: Option<&str>,
    ) -> Result<Result<Opened, SubmitRefusal>>;

    /// 125: push each of `worktrees`' branches and open or update its
    /// pull request into `branch`, in order, each marked one of the group
    /// and titled by its own of `subjects`, which are empty or one per
    /// worktree, or none of them when any worktree is refused; then, for
    /// more than one, mark each ready and queue them all in one write, or,
    /// when any falls short of its conditions, leave each a draft and
    /// queue none.
    fn open_group(
        &self,
        _seams: Seams<'_>,
        _clock: &dyn Clock,
        _worktrees: &[String],
        _subjects: &[String],
        branch: &str,
    ) -> Result<Result<Vec<Opened>, GroupRefused>> {
        anyhow::bail!("{branch} takes no pull requests")
    }

    /// The pull request `worktree`'s work is in, open or landed.
    fn find(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
        worktree: &str,
    ) -> Result<Option<u64>>;

    /// Where pull request `number` into `branch` stands, as the last
    /// intake read it.
    fn follow(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
        number: u64,
    ) -> Result<Followed>;

    /// 137, design D5: the pull requests into `branch` this host follows
    /// whose submissions its queue does not hold at their heads, each read
    /// as the host's one bound allows (design D4), with the host the
    /// branch's intake mark names; one read merged or closed is given once
    /// and followed no longer (design D3). `None` where it follows none, or
    /// the branch's records are on its remote (26, 80).
    fn across(
        &self,
        _seams: Seams<'_>,
        _clock: &dyn Clock,
        _branch: &str,
    ) -> Result<Option<Across>> {
        Ok(None)
    }

    /// 137, design D5: as [`PullRequests::across`], as the host last kept
    /// them, reading GitHub for none of them and following none no longer:
    /// for a reader's document, which reads them again only once what the
    /// host keeps of them, or the intake mark, moved (129; rules core.25,
    /// core.31).
    fn kept_across(
        &self,
        _seams: Seams<'_>,
        _clock: &dyn Clock,
        _branch: &str,
    ) -> Result<Option<Across>> {
        Ok(None)
    }

    /// The pull request a submission's place names, where it names one.
    fn request_of(&self, _place: &str) -> Option<u64> {
        None
    }

    /// 137: whether `sub` was withdrawn by its host's hand-over of its
    /// branch's pull requests to another host.
    fn handed_over(&self, _sub: &Submission) -> bool {
        false
    }
}

/// No branch takes pull requests.
impl PullRequests for () {
    fn branches(&self, _: Seams<'_>) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn intake(&self, _: Seams<'_>, _: &dyn Clock, _: &str, _: Taker) -> Result<bool> {
        Ok(false)
    }

    fn open(
        &self,
        _: Seams<'_>,
        _: &dyn Clock,
        _: &SubmitHead,
        branch: &str,
        _: Option<&str>,
    ) -> Result<Result<Opened, SubmitRefusal>> {
        anyhow::bail!("{branch} takes no pull requests")
    }

    fn find(&self, _: Seams<'_>, _: &dyn Clock, _: &str, _: &str) -> Result<Option<u64>> {
        Ok(None)
    }

    fn follow(&self, _: Seams<'_>, _: &dyn Clock, branch: &str, _: u64) -> Result<Followed> {
        anyhow::bail!("{branch} takes no pull requests")
    }
}

/// Take in `branch`'s pull requests before its round. One that cannot be
/// read is said and the round goes on, landing what is queued; a failure
/// that stands is said once, as a round's is, until intake reads again or
/// it fails in other words (26, 133). A spent
/// allowance of GitHub's calls is the error returned: one the retry
/// policy stopped on at once, a round's, is said once for its pause on
/// `out`, or on stderr under `--json` (133, 134; ADR 0071); one it waited
/// out to its end, a wait's deadline or Ctrl-C, carries how long, and
/// was said on stderr as the wait began.
fn intake(
    pulls: &dyn PullRequests,
    seams: Seams<'_>,
    clock: &dyn Clock,
    branch: &str,
    taker: Taker,
    json: bool,
    out: &mut dyn Write,
) -> Option<anyhow::Error> {
    thread_local! {
        /// The end of the last pause a round said it passed by.
        static SAID: std::cell::Cell<Option<chrono::DateTime<chrono::Utc>>> =
            const { std::cell::Cell::new(None) };
        /// Each branch's intake failure as last said, while it stands.
        static FAILED: std::cell::RefCell<BTreeMap<String, String>> =
            std::cell::RefCell::default();
    }
    let e = match pulls.intake(seams, clock, branch, taker) {
        Ok(_) => {
            FAILED.with_borrow_mut(|f| f.remove(branch));
            return None;
        }
        Err(e) => e,
    };
    let Some(limited) = retry::limited(&e) else {
        let words = format!("sluice: warning: {e:#}");
        let said = FAILED.with_borrow_mut(|f| f.insert(branch.to_string(), words.clone()));
        if said.as_ref() != Some(&words) {
            telemetry::warn(&words);
        }
        return None;
    };
    let waited = e.downcast_ref::<retry::Spent>().is_some();
    if !waited && SAID.get() != Some(limited.until) {
        SAID.set(Some(limited.until));
        let line = format!("{branch}'s pull requests passed by: {limited}");
        if json || writeln!(out, "{line}").is_err() {
            telemetry::warn(&format!("sluice: {line}"));
        }
    }
    Some(e)
}

#[derive(Subcommand, Debug)]
pub enum BranchCommand {
    /// Make the branch at another branch's current revision, only if it
    /// is absent and that revision's policy gives it a gate.
    #[command(after_help = "Example:\n  $ sluice branch create feature/x --from main")]
    Create {
        /// The branch to make.
        branch: String,
        /// The branch sluice lands on to make it from.
        #[arg(long)]
        from: String,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Remove the branch, only once nothing is queued on it and its
    /// revision has landed into another branch.
    #[command(after_help = "Example:\n  $ sluice branch delete feature/x --into main")]
    Delete {
        /// The branch to remove.
        branch: String,
        /// The branch its work landed into.
        #[arg(long, default_value = "main")]
        into: String,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Accept the revision a held branch is read at, which lacks its last
    /// landing, as the one it stands on: the next round forms the queue on
    /// it. The operator's to do, never a session's.
    #[command(after_help = "Example:\n  $ sluice branch accept feature/x 10e4791")]
    Accept {
        /// The held branch.
        branch: String,
        /// The revision it is held at, as `sluice status <branch>` names it.
        revision: String,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ForgeCommand {
    /// Whether the host's [forge] identity is set up so only sluice may
    /// move the branch on its remote; exits 1 naming what is not.
    #[command(after_help = "Examples:\n  $ sluice forge check\n  $ sluice forge check main")]
    Check {
        /// The branch; the one checked out here by default.
        branch: Option<String>,
        /// One JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
}

/// What a waiting command does between rounds.
pub trait Pause {
    /// Wait for the next round; `false` when the wait was stopped.
    fn pause(&self) -> bool;
}

/// Set by Ctrl-C once [`stop_on_interrupt`] is in force.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupted(_: nix::libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Ctrl-C ends a wait and nothing else: the submission stays queued (5)
/// and its run, in a session of its own, goes on.
pub fn stop_on_interrupt() -> Result<()> {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
    let action = SigAction::new(
        SigHandler::Handler(interrupted),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: the handler only stores to an atomic.
    unsafe { sigaction(Signal::SIGINT, &action) }?;
    Ok(())
}

/// A pause between rounds that skips its time and moves its own clock on
/// by it, a clock nothing else moves: a test follows a wait over minutes
/// in a moment, with every time it reads, a deadline's and the host's
/// interval's among them, as the wait would have read it.
pub struct Skipped {
    every: std::time::Duration,
    start: chrono::DateTime<chrono::Utc>,
    moved: std::cell::Cell<std::time::Duration>,
}

impl Skipped {
    pub fn new(every: std::time::Duration) -> Skipped {
        Skipped {
            every,
            start: chrono::Utc::now(),
            moved: std::cell::Cell::default(),
        }
    }
}

impl Pause for Skipped {
    fn pause(&self) -> bool {
        self.moved.set(self.moved.get() + self.every);
        !INTERRUPTED.load(Ordering::SeqCst)
    }
}

impl Clock for Skipped {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.start + chrono::TimeDelta::from_std(self.moved.get()).unwrap_or_default()
    }
}

/// How long a command pauses between its reads, the tick its reader names
/// (64, 129): a watching overview reads about once a second; a run, a
/// waiting submit or `sluice wait` rounds every three.
pub fn tick(command: Option<&Command>) -> Duration {
    match command {
        Some(Command::Overview { .. }) => Duration::from_secs(1),
        _ => Duration::from_secs(3),
    }
}

pub struct Sleep(pub std::time::Duration);

impl Pause for Sleep {
    fn pause(&self) -> bool {
        let until = std::time::Instant::now() + self.0;
        while std::time::Instant::now() < until {
            if INTERRUPTED.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !INTERRUPTED.load(Ordering::SeqCst)
    }
}

/// The exit of a wait stopped by Ctrl-C.
pub const STOPPED: i32 = 130;

fn requester(by: &str) -> Requester {
    match by {
        "planner" => Requester::Planner,
        "operator" => Requester::Operator,
        agent => Requester::Session(agent.into()),
    }
}

impl Cli {
    /// 151: what this command was given of telemetry, by its flags or
    /// its `SLUICE_OTEL*` variables, which choose for it alone over the
    /// host's `[telemetry]` (record 0106; design D2).
    pub fn choices(&self) -> telemetry::Choices {
        telemetry::Choices {
            otel: self.otel,
            service: self.otel_service_name.clone(),
            logs: self.otel_logs,
            logs_limit: self.otel_logs_limit,
        }
    }

    /// Whether the command was asked for JSON, so that even a failure
    /// before or outside it is said as JSON.
    pub fn json(&self) -> bool {
        match &self.command {
            None => self.json,
            Some(
                Command::Submit { json, .. }
                | Command::Wait { json, .. }
                | Command::Inbox { json, .. }
                | Command::Confirm { json, .. }
                | Command::Status { json, .. }
                | Command::Overview { json, .. }
                | Command::Exec { json, .. }
                | Command::Equip { json, .. }
                | Command::Preview { json, .. }
                | Command::Previews { json }
                | Command::Step { json, .. }
                | Command::Amend { json, .. }
                | Command::Withdraw { json, .. }
                | Command::SetAside { json, .. }
                | Command::Release { json, .. }
                | Command::Protect { json, .. }
                | Command::Forge {
                    command: ForgeCommand::Check { json, .. },
                }
                | Command::Branch {
                    command:
                        BranchCommand::Create { json, .. }
                        | BranchCommand::Delete { json, .. }
                        | BranchCommand::Accept { json, .. },
                },
            ) => *json,
            Some(_) => false,
        }
    }
}

/// What `sluice run` carries from one pass to the next, whatever binding
/// each pass runs under (135; design D3): the passes counted toward its
/// `--rounds`, the repository's driver's lock and its reader (129),
/// what its document last asked, whether it said that another run drives
/// the repository, the holds and the refusals it said, the policies it
/// read by revision (129; rule core.31), and whether the host's systems
/// were checked (87).
pub struct Driving {
    rounds: Option<usize>,
    /// The tick the reader's document names (64).
    tick: Duration,
    n: usize,
    checked: bool,
    /// Where the host's readers keep their files, and the repository as
    /// they name it.
    place: Option<(std::path::PathBuf, String)>,
    driver: Option<Driver>,
    /// 135: the driver's lock handed over by the `sluice run` this one
    /// started afresh from, adopted at the first pass before the lock
    /// would be taken (design D4, D6).
    handed: Option<OwnedFd>,
    reader: Option<Reader>,
    /// 129: what its document last asked of whether the repository has
    /// sluice.
    asked: Option<overview::Asked>,
    said_beside: bool,
    /// 121: each branch's hold as last said, so it is said once.
    said: BTreeMap<String, Option<(Rev, Rev)>>,
    /// 26, 121: the words each branch passed by was last said with.
    passed: BTreeMap<String, String>,
    /// 129: each policy read, by revision, kept across passes and
    /// bindings, since a policy at a revision is that revision's bytes.
    policies: sluice_core::Policies,
}

impl Driving {
    /// A `sluice run` that has driven no pass yet, ending after `rounds`
    /// passes where it names a bound.
    pub fn new(rounds: Option<usize>) -> Driving {
        Driving {
            rounds,
            tick: tick(Some(&Command::Run { rounds })),
            n: 0,
            checked: false,
            place: None,
            driver: None,
            handed: None,
            reader: None,
            asked: None,
            said_beside: false,
            said: BTreeMap::new(),
            passed: BTreeMap::new(),
            policies: sluice_core::Policies::default(),
        }
    }

    /// What this run hands to itself started afresh (135; record 0107;
    /// design D4, D6): the passes counted, a duplicate of the driver's
    /// lock left open across the start, and what it said. The driver is
    /// kept, so a hand-over that cannot start still drives.
    pub fn hand_over(&self) -> Result<Carried> {
        Ok(Carried {
            passes: self.n,
            driver: self.driver.as_ref().map(Driver::hand_over).transpose()?,
            said: self.said.clone(),
            passed: self.passed.clone(),
            said_beside: self.said_beside,
        })
    }

    /// A `sluice run` started afresh from one that handed it `carried`,
    /// ending once `rounds` passes, those already driven among them, where
    /// it names a bound. Its first pass adopts the driver's lock carried
    /// before it would take the lock, and it says again nothing the run
    /// said (135; record 0107; design D4, D6).
    pub fn handed(rounds: Option<usize>, carried: Carried) -> Driving {
        Driving {
            n: carried.passes,
            handed: carried.driver,
            said: carried.said,
            passed: carried.passed,
            said_beside: carried.said_beside,
            ..Driving::new(rounds)
        }
    }
}

/// What a `sluice run` hands to itself started afresh, as [`Driving`]
/// keeps it (135; record 0107; design D4, D6): the passes counted toward
/// its `--rounds`, the driver's lock as a descriptor open across the
/// start, where it held it, and the holds, the refusals and whether it
/// said another run drives the repository, so none is said again.
#[derive(Debug, Default)]
pub struct Carried {
    pub passes: usize,
    pub driver: Option<OwnedFd>,
    pub said: BTreeMap<String, Option<(Rev, Rev)>>,
    pub passed: BTreeMap<String, String>,
    pub said_beside: bool,
}

/// What a pass of `sluice run` ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driven {
    /// It paused, and the next pass is due.
    Next,
    /// The run ends with this exit: its `--rounds` reached, or its pause
    /// stopped.
    Ended(i32),
}

/// One pass of `sluice run` over the repository the seams bind, and the
/// pause after it (135; design D3): the host's systems checked before the
/// first pass `driving` runs (87), then the repository's driver's lock
/// taken or kept, every branch's round run and the reader's document
/// written (129). Its caller may bind the repository afresh between two
/// calls, and the next pass runs wholly under the new binding (rule
/// core.54).
pub fn drive(
    driving: &mut Driving,
    seams: Seams<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<Driven> {
    let s = Sluice::new(seams, clock)
        .as_peer(host.peer())
        .keeping(&driving.policies);
    if !driving.checked {
        s.check_host_systems()?;
        driving.checked = true;
    }
    // 135: the repository's one driver on the host, and so its reader
    // (129): a run that finds another live drives nothing, and looks again
    // at each pass.
    let (readers, repository) = match &driving.place {
        Some(place) => place.clone(),
        None => {
            let place = (host.readers()?, host.repository()?);
            driving.place = Some(place.clone());
            place
        }
    };
    // 26: the branches this pass passed by, with their words; a
    // bounded run whose last pass passed one by ends as a step
    // of that branch would.
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    // 135: a run started afresh holds the lock it was handed, with no
    // moment let go, where it is this repository's (design D4, D6).
    if let Some(fd) = driving.handed.take() {
        driving.driver = Driver::adopt(fd, &readers, &repository)?;
    }
    if driving.driver.is_none() {
        driving.driver = Driver::take(&readers, &repository)?;
    }
    if driving.driver.is_some() {
        // Its part in reading: it drives, so the place is its,
        // whatever lease a newer watch holds, and it takes none. A
        // lock it could not try leaves it following, as a watch.
        match &mut driving.reader {
            Some(r) => {
                let _ = r.step(false, Drives::Itself);
            }
            None => {
                driving.reader = Some(Reader::take_given(
                    &readers,
                    &repository,
                    host.version(),
                    Drives::Itself,
                )?)
            }
        }
        // 129: what the last pass read by revision and this one
        // does not use is forgotten.
        s.next_pass();
        host.next_pass();
        // A read whose start it could not mark leaves a forecast
        // reading past this reader, which decides nothing (64).
        if let Some(Reader::Lead(lead)) = &driving.reader {
            let _ = lead.begin();
        }
        // 129: each remote that keeps branches' records asked
        // once, first; a refusal is its branches' alone, read as
        // last fetched.
        if let Err(e) = seams.state.refresh() {
            telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
        // 97, previews-1 101: every pass, whether or not a branch
        // has a queue.
        exec::tell_abandoned(out, false, &s.release_abandoned()?)?;
        preview::tell_dropped(out, false, &s.release_unusable_previews()?)?;
        let mut branches = seams.state.branches()?;
        match pulls.branches(seams) {
            Ok(b) => branches.extend(b),
            Err(e) => telemetry::warn(&format!("sluice: warning: {e:#}")),
        }
        branches.sort();
        branches.dedup();
        for branch in branches {
            intake(pulls, seams, clock, &branch, Taker::Driver, false, out);
            // 26, rule core.14: a branch whose records' read or
            // round fails is that branch's alone; the pass goes
            // on to every other branch. 129: one with nothing in
            // play costs the pass its records' read alone.
            let ran = s.in_play(&branch).and_then(|in_play| match in_play {
                false => Ok(None),
                true => s.round(&branch).map(Some),
            });
            let round = match ran {
                Ok(round) => {
                    driving.passed.remove(&branch);
                    round
                }
                // Ctrl-C reaching the git a branch's read runs is no
                // refusal of that branch's.
                Err(_) if INTERRUPTED.load(Ordering::SeqCst) => None,
                Err(e) => {
                    let words = format!("{e:#}");
                    tell_passed(out, &branch, &words, &mut driving.passed)?;
                    failed.insert(branch.clone(), words);
                    None
                }
            };
            if let Some(round) = round {
                exec::tell_abandoned(out, false, &round.released)?;
                preview::tell_dropped(out, false, &round.previews)?;
                exec::tell_releases(out, false, &round.releases)?;
                if matches!(round.step, Step::Stepped | Step::Held) {
                    let said = driving.said.entry(branch.clone()).or_default();
                    tell_hold(out, &branch, round.held.as_ref(), said)?;
                }
            }
        }
        // 129: the reader's document every watch and wait on the
        // host follows, each pass, queue or none, read as the
        // pass's own ask left the records and naming its tick
        // (64); a read it could not make is its document, in the
        // words it was refused with (133), and the pass goes on.
        // Ctrl-C reaching the git a read runs is no failure of the
        // queues'.
        if let Some(Reader::Lead(lead)) = &mut driving.reader {
            let doc =
                overview::read_held(&s, host, clock, Follows::Kept(pulls), &mut driving.asked)
                    .unwrap_or_else(|e| told::document(Kind::Internal, &format!("{e:#}"), &[]));
            let doc = overview::ticked(doc, driving.tick);
            if !INTERRUPTED.load(Ordering::SeqCst) {
                // The last document stands for a write that failed
                // until one succeeds.
                let _ = lead.write(&doc);
            }
        }
    } else if !driving.said_beside {
        telemetry::warn(
            "sluice: another sluice run drives this repository on this host; \
             this one runs nothing while it does",
        );
        driving.said_beside = true;
    }
    driving.n += 1;
    if driving.rounds.is_some_and(|r| driving.n >= r) {
        // 26: one line per branch, as `main` writes a failed step's, said
        // whether or not stdout said it this pass.
        for (branch, words) in &failed {
            telemetry::error(&format!("sluice: {branch}: {words}"));
        }
        return Ok(Driven::Ended(if failed.is_empty() { 0 } else { INTERNAL }));
    }
    if !pause.pause() {
        return Ok(Driven::Ended(STOPPED));
    }
    Ok(Driven::Next)
}

/// Run one command; returns its exit code.
pub fn run(
    cli: Cli,
    seams: Seams<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    let s = Sluice::new(seams, clock).as_peer(host.peer());
    let tick = tick(cli.command.as_ref());
    // 124: the hook reads the policy only as far as guarding needs.
    let s = match &cli.command {
        Some(Command::Hook { .. }) => s.guarding(),
        _ => s,
    };
    let Some(command) = cli.command else {
        return home::run(&s, host, &cli.worktree, cli.json, out);
    };
    match command {
        Command::Submit {
            branch,
            after,
            priority,
            no_wait,
            subject,
            check,
            head,
            status,
            worktree,
            json,
            confirm,
            confirm_landing,
        } => {
            let confirms = Confirms::of(confirm, confirm_landing);
            // 124, D1: a submit by name reads no worktree, and has no last
            // submission of one to report.
            if head.is_some() && !check {
                let why = if !worktree.is_empty() {
                    Some(
                        "--head names what is submitted, from anywhere in the repository; give \
                         no --worktree with it",
                    )
                } else if status {
                    Some(
                        "--status reads a worktree's last submission; a branch's are in sluice \
                         status <target> --all",
                    )
                } else if subject.len() > 1 {
                    Some("--head submits one branch; give one --subject with it, or none")
                } else {
                    None
                };
                if let Some(why) = why {
                    writeln!(out, "sluice submit: {why}")?;
                    return Ok(USAGE);
                }
            }
            let worktree = if worktree.is_empty() {
                vec![".".to_string()]
            } else {
                worktree
            };
            // 125: several worktrees are one act, which neither forecasts
            // nor reports one worktree's last submission.
            if worktree.len() > 1 && (check || status) {
                let flag = if check { "--check" } else { "--status" };
                writeln!(
                    out,
                    "sluice submit: {flag} takes one worktree; give --worktree once with it"
                )?;
                return Ok(USAGE);
            }
            // C.5 pull request, 23: a subject is one line; 125: a group
            // gives one per worktree, in their order, or none.
            let why = if subject.iter().any(|s| s.trim().is_empty()) {
                Some("--subject is empty; a subject is one line saying what lands".to_string())
            } else if subject.iter().any(|s| s.contains(['\n', '\r'])) {
                Some("a subject is one line; --subject holds a line break".to_string())
            } else if !subject.is_empty() && subject.len() != worktree.len() {
                let given =
                    |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
                Some(format!(
                    "{} for {}; give one --subject per --worktree, in their order, or none",
                    given(subject.len(), "subject"),
                    given(worktree.len(), "worktree")
                ))
            } else {
                None
            };
            if let Some(why) = why {
                writeln!(out, "sluice submit: {why}")?;
                return Ok(USAGE);
            }
            if worktree.len() > 1 {
                let req = GroupRequest {
                    branch,
                    heads: worktree.into_iter().map(SubmitHead::Worktree).collect(),
                    after,
                    priority,
                    subjects: subject,
                };
                if takes_pulls(&s, &req.branch)? {
                    return submit_pull_group(
                        &s, host, pulls, clock, req, no_wait, json, pause, out,
                    );
                }
                return submit_group(
                    &s, host, pushers, pulls, clock, req, no_wait, json, pause, tick, out,
                );
            }
            let worktree = worktree.into_iter().next().unwrap_or_else(|| ".".into());
            let subject = subject.into_iter().next();
            if check {
                return forecast(
                    &s,
                    host,
                    pushers,
                    &worktree,
                    head.as_deref(),
                    &branch,
                    &after,
                    json,
                    out,
                );
            }
            // 124: a branch sluice lands on, submitted by name; to a branch
            // that takes pull requests, by the request from it (C.5).
            if let Some(named) = head {
                if takes_pulls(&s, &branch)? {
                    let mode = PullMode::Open {
                        no_wait,
                        subject: subject.as_deref(),
                    };
                    let named = SubmitHead::Branch(named);
                    return submit_pull(
                        &s, host, pulls, clock, &named, &branch, mode, json, confirms, pause, tick,
                        out,
                    );
                }
                return submit(
                    &s,
                    host,
                    pushers,
                    pulls,
                    clock,
                    SubmitRequest {
                        head: SubmitHead::Branch(named),
                        branch,
                        after,
                        priority,
                        subject,
                    },
                    no_wait,
                    json,
                    confirms,
                    pause,
                    tick,
                    out,
                );
            }
            if takes_pulls(&s, &branch)? {
                let mode = if status {
                    PullMode::Status
                } else {
                    PullMode::Open {
                        no_wait,
                        subject: subject.as_deref(),
                    }
                };
                let worktree = SubmitHead::Worktree(worktree);
                return submit_pull(
                    &s, host, pulls, clock, &worktree, &branch, mode, json, confirms, pause, tick,
                    out,
                );
            }
            if status {
                return submitted(&s, clock, &worktree, &branch, json, out);
            }
            submit(
                &s,
                host,
                pushers,
                pulls,
                clock,
                SubmitRequest {
                    head: SubmitHead::Worktree(worktree),
                    branch,
                    after,
                    priority,
                    subject,
                },
                no_wait,
                json,
                confirms,
                pause,
                tick,
                out,
            )
        }
        Command::Wait {
            target,
            timeout,
            worktree,
            json,
            confirm,
            confirm_landing,
            branch,
        } => {
            let confirms = Confirms::of(confirm, confirm_landing);
            let deadline = timeout.map(|t| {
                clock.now() + chrono::Duration::from_std(t).unwrap_or(chrono::Duration::MAX)
            });
            let number = target.parse::<u64>();
            if number.is_err() && branch.is_some() {
                writeln!(out, "sluice wait: {}", branch_goes_with_a_number(&target))?;
                return Ok(USAGE);
            }
            let sub = match number {
                Ok(id) => match lookup(&s, id, branch.as_deref())? {
                    Lookup::Found(sub) => sub,
                    Lookup::Unfound(unread) if unread.is_empty() => {
                        let (text, next) = no_such(id, branch.as_deref());
                        told::tell(out, json, Kind::NotFound, &text, &next)?;
                        return Ok(FAILED);
                    }
                    Lookup::Unfound(unread) => return unfound(id, &unread, json, out),
                    // A wait's 2 is a conflict: the refusal exits 64.
                    Lookup::Several(several) => {
                        return refuse_several(id, "sluice wait", &several, json, USAGE, out)
                    }
                },
                Err(_) if takes_pulls(&s, &target)? => {
                    let mode = PullMode::Wait { deadline };
                    let worktree = SubmitHead::Worktree(worktree);
                    return submit_pull(
                        &s, host, pulls, clock, &worktree, &target, mode, json, confirms, pause,
                        tick, out,
                    );
                }
                Err(_) => match last_submitted(&s, &worktree, &target)? {
                    Ok(sub) => sub,
                    Err(from) => {
                        let text = format!("nothing submitted from {from} to {target}");
                        let next = [format!("sluice submit {target}")];
                        told::tell(out, json, Kind::NotFound, &text, &next)?;
                        return Ok(FAILED);
                    }
                },
            };
            let intake = takes_pulls(&s, &sub.branch)?.then_some(pulls);
            let waiting = Waiting {
                host,
                clock,
                pulls: intake,
                follows: pulls,
                deadline,
                json,
                pause,
                tick,
                copy: None,
                worktree: Some(&worktree),
                confirms,
                named: branch.as_deref(),
            };
            await_submission(&s, &waiting, sub, None, out)
        }
        Command::Inbox {
            worktree,
            json,
            confirm,
            new,
        } => inbox(&s, &worktree, json, confirm, new, out),
        Command::Confirm {
            keys,
            who,
            worktree,
            json,
        } => confirm(&s, &keys, who.is_some(), &worktree, json, out),
        Command::Step { branch, json } => {
            intake(pulls, seams, clock, &branch, Taker::Step, json, out);
            let round = s.round(&branch)?;
            exec::tell_abandoned(out, json, &round.released)?;
            preview::tell_dropped(out, json, &round.previews)?;
            exec::tell_releases(out, json, &round.releases)?;
            if json {
                let outcome = match round.step {
                    Step::Stepped => "stepped",
                    Step::Busy => "busy",
                    Step::Led => "led",
                    Step::Contended => "contended",
                    Step::Held => "held",
                };
                let mut doc = serde_json::json!({
                    "branch": branch,
                    "outcome": outcome,
                    "released": exec::abandoned_json(&round.released),
                    "previews_released": preview::dropped_json(&round.previews),
                    "releases": exec::releases_json(&round.releases),
                });
                if let Some(hold) = &round.held {
                    doc["held"] = held_json(hold);
                }
                writeln!(out, "{doc}")?;
                return Ok(0);
            }
            if let Some(hold) = &round.held {
                writeln!(out, "{}", held_line(&branch, hold))?;
                return Ok(0);
            }
            let word = match round.step {
                Step::Stepped => "stepped",
                Step::Busy => "busy: another round holds the lock",
                Step::Led => "led: another host keeps the branch, and its lease has not passed",
                Step::Contended => "contended: the records moved; nothing done",
                Step::Held => "held",
            };
            writeln!(out, "{branch}: {word}")?;
            Ok(0)
        }
        Command::Run { rounds } => {
            let mut driving = Driving::new(rounds);
            loop {
                if let Driven::Ended(code) =
                    drive(&mut driving, seams, host, pulls, clock, pause, out)?
                {
                    return Ok(code);
                }
            }
        }
        Command::Status {
            target: None,
            branch: Some(_),
            ..
        } => {
            writeln!(
                out,
                "sluice status: --branch names the branch of a submission's number; give the \
                 number with it"
            )?;
            Ok(USAGE)
        }
        Command::Status {
            target: None, json, ..
        } => {
            let places = s.session_places()?;
            let held = s.held()?;
            if json {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "worktrees": sessions_json(&places, &held),
                    }))?
                )?;
            } else {
                write_worktrees(&places, out)?;
                write_held(&held, out)?;
            }
            Ok(0)
        }
        Command::Status {
            target: Some(target),
            branch: Some(_),
            ..
        } if target.parse::<u64>().is_err() => {
            writeln!(out, "sluice status: {}", branch_goes_with_a_number(&target))?;
            Ok(USAGE)
        }
        Command::Status {
            target: Some(target),
            all,
            json,
            branch,
        } => match target.parse::<u64>() {
            Ok(id) => match lookup(&s, id, branch.as_deref())? {
                Lookup::Unfound(unread) if unread.is_empty() => {
                    let (text, next) = no_such(id, branch.as_deref());
                    told::tell(out, json, Kind::NotFound, &text, &next)?;
                    Ok(1)
                }
                Lookup::Unfound(unread) => unfound(id, &unread, json, out),
                Lookup::Several(several) => {
                    refuse_several(id, "sluice status", &several, json, 2, out)
                }
                Lookup::Found(sub) if json => {
                    let mut doc = with_waiting(&s, &sub, decided_document(&sub, None)?);
                    // 51, D13: a decided submission's delivery.
                    if let Some(d) = s.delivery_of(&sub)? {
                        doc["delivery"] = d.to_json();
                    }
                    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                    Ok(0)
                }
                Lookup::Found(sub) => {
                    if sub.id != id {
                        writeln!(
                            out,
                            "submission {id} is #{} now: {}'s queue moved to its remote",
                            sub.id, sub.branch
                        )?;
                    }
                    let branch = sub.branch.clone();
                    let pusher = pushers.pusher(&branch)?;
                    let room = s.room(host.reserved())?;
                    let status = s.status(&branch)?;
                    let names = status_names(&status, std::slice::from_ref(&sub));
                    let testing =
                        progress::lines(&s, &branch, Some(sub.id), clock.now(), Some(&names))?;
                    // 124, D10: no session of its own moves a submission by
                    // name on, so its status names its way on.
                    let ways = if sub.from.by_name() {
                        ways_on(&s, &sub, &branch)?
                    } else {
                        Vec::new()
                    };
                    write_status(
                        &status,
                        pusher.as_ref(),
                        &testing,
                        &[sub],
                        &names,
                        Some(&room),
                        out,
                    )?;
                    for w in ways {
                        writeln!(out, "{w}")?;
                    }
                    Ok(0)
                }
            },
            Err(_) => {
                let status = s.status(&target)?;
                if status.is_unknown() {
                    let known: Vec<String> =
                        s.overview(None)?.into_iter().map(|b| b.branch).collect();
                    let text = match known.as_slice() {
                        [] => format!("no branch {target}, and no branch has records here"),
                        _ => format!(
                            "no branch {target}; branches with records: {}",
                            known.join(", ")
                        ),
                    };
                    let next: Vec<String> = known
                        .iter()
                        .map(|b| format!("sluice status {b}"))
                        .chain(["sluice overview".to_string()])
                        .collect();
                    told::tell(out, json, Kind::NotFound, &text, &next)?;
                    return Ok(FAILED);
                }
                // The queue by default; its history, read from the finished
                // records too, only when asked (131).
                let shown: Vec<Submission> = if all {
                    s.history(&target)?
                } else {
                    in_queue(&status.submissions).cloned().collect()
                };
                // 137, design D5: the pull requests this host follows that
                // another host takes in, as that host shows them; one that
                // cannot be read is warned of, and the queue still shown.
                let across = pulls.across(s.seams(), clock, &target).unwrap_or_else(|e| {
                    telemetry::warn(&format!("sluice: warning: {e:#}"));
                    None
                });
                // 51, D13: what the branch's readers are owed, shown after
                // its queue.
                let owed = s.owed_on_branch(&target)?;
                if json {
                    let mut doc = status_json(&status, &shown)?;
                    // 84, record 0074: what each combination waits for.
                    doc["waiting"] = progress::waiting_json(&s.branch_view_of(&target)?);
                    doc["owed"] = owed_json(&owed);
                    if let Some(a) = &across {
                        doc["taken_in_by"] = serde_json::to_value(&a.taken_in_by)?;
                        doc["pull_requests"] = serde_json::to_value(&a.pull_requests)?;
                    }
                    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                } else {
                    let pusher = pushers.pusher(&target)?;
                    let room = s.room(host.reserved())?;
                    let names = status_names(&status, &shown);
                    let testing = progress::lines(&s, &target, None, clock.now(), Some(&names))?;
                    write_status(
                        &status,
                        pusher.as_ref(),
                        &testing,
                        &shown,
                        &names,
                        Some(&room),
                        out,
                    )?;
                    let earlier =
                        (status.submissions.len() + status.finished).saturating_sub(shown.len());
                    if shown.is_empty() {
                        writeln!(out, "nothing in the queue")?;
                    }
                    if earlier > 0 {
                        writeln!(
                            out,
                            "{earlier} no longer in the queue: sluice status {target} --all"
                        )?;
                    }
                    if let Some(a) = &across {
                        writeln!(out, "{}", a.heading(&target))?;
                        for row in &a.pull_requests {
                            writeln!(out, "  {}", a.line(row))?;
                        }
                    }
                    write_owed(&owed, out)?;
                }
                Ok(0)
            }
        },
        Command::Overview { json, watch } => {
            overview::run(&s, host, clock, pulls, json, watch, pause, tick, out)
        }
        Command::Amend {
            id,
            after,
            priority,
            by,
            json,
            branch,
        } => match match &branch {
            Some(b) => s.amend_on(b, id, &requester(&by), after, priority)?,
            None => s.amend(id, &requester(&by), after, priority)?,
        } {
            Ok(()) if json => {
                // Record 0114: another branch may take the number between
                // the amend and this read, which then finds none.
                let doc = match state(&s, id, branch.as_deref())? {
                    Some(sub) => serde_json::json!({
                        "id": id, "after": sub.after, "priority": sub.priority,
                    }),
                    None => serde_json::json!({ "id": id }),
                };
                writeln!(out, "{doc}")?;
                Ok(0)
            }
            Ok(()) => {
                writeln!(out, "amended #{id}")?;
                Ok(0)
            }
            Err(AmendRefusal::NoSuchSubmission(unread)) if !unread.is_empty() => {
                unfound(id, &unread, json, out)
            }
            Err(AmendRefusal::Several(several)) => {
                refuse_several(id, "sluice amend", &several, json, 2, out)
            }
            Err(r) => {
                let (why, next) = match r {
                    AmendRefusal::NoSuchSubmission(_) | AmendRefusal::Several(_) => {
                        no_submission(id, branch.as_deref())
                    }
                    AmendRefusal::NotYours => (
                        format!(
                            "#{id} is not {by}'s to change: {}",
                            owner(&s, id, branch.as_deref())?
                        ),
                        owner_next(
                            &s,
                            id,
                            branch.as_deref(),
                            &asked("sluice amend", id, branch.as_deref()),
                        )?,
                    ),
                    AmendRefusal::UnderTest => {
                        let status = asked("sluice status", id, branch.as_deref());
                        let withdraw = asked("sluice withdraw", id, branch.as_deref());
                        (
                            format!(
                                "#{id} is under test; its order is fixed: {status} follows it, \
                                 or {withdraw} takes it out"
                            ),
                            vec![status, withdraw],
                        )
                    }
                    AmendRefusal::NotQueued(st) => {
                        let status = asked("sluice status", id, branch.as_deref());
                        (
                            format!("#{id} is {}, past changing: {status}", st.word()),
                            vec![status],
                        )
                    }
                    AmendRefusal::UnknownAfter(ids) => {
                        let queue = match state(&s, id, branch.as_deref())? {
                            Some(sub) => format!("sluice status {}", sub.branch),
                            None => "sluice overview".into(),
                        };
                        let text = format!(
                            "--after names no queued submission: {}; {queue} lists the queue",
                            hashes(&ids, None, &mut BTreeSet::new())
                        );
                        (text, vec![queue])
                    }
                };
                told::tell(out, json, Kind::Refused, &format!("refused: {why}"), &next)?;
                Ok(2)
            }
        },
        Command::Withdraw {
            id,
            by,
            json,
            branch,
        } => match match &branch {
            Some(b) => s.withdraw_on(b, id, &requester(&by))?,
            None => s.withdraw(id, &requester(&by))?,
        } {
            Ok(withdrew) => {
                let already = withdrew == Withdrew::Already;
                if json {
                    let mut doc = serde_json::json!({ "id": id, "state": "withdrawn" });
                    if already {
                        doc["already"] = true.into();
                    }
                    writeln!(out, "{doc}")?;
                } else if already {
                    writeln!(out, "#{id} was already withdrawn")?;
                } else {
                    writeln!(out, "withdrew #{id}")?;
                }
                Ok(0)
            }
            Err(WithdrawRefusal::NoSuchSubmission(unread)) if !unread.is_empty() => {
                unfound(id, &unread, json, out)
            }
            Err(WithdrawRefusal::Several(several)) => {
                refuse_several(id, "sluice withdraw", &several, json, 2, out)
            }
            Err(r) => {
                let (why, next) = match r {
                    WithdrawRefusal::NoSuchSubmission(_) | WithdrawRefusal::Several(_) => {
                        no_submission(id, branch.as_deref())
                    }
                    WithdrawRefusal::NotYours => (
                        format!(
                            "#{id} is not {by}'s to withdraw: {}",
                            owner(&s, id, branch.as_deref())?
                        ),
                        owner_next(
                            &s,
                            id,
                            branch.as_deref(),
                            &asked("sluice withdraw", id, branch.as_deref()),
                        )?,
                    ),
                    WithdrawRefusal::Final(st) => {
                        let status = asked("sluice status", id, branch.as_deref());
                        (
                            format!("#{id} is {}, out of the queue already: {status}", st.word()),
                            vec![status],
                        )
                    }
                };
                told::tell(out, json, Kind::Refused, &format!("refused: {why}"), &next)?;
                Ok(2)
            }
        },
        Command::SetAside {
            task,
            at,
            hash,
            reason,
            json,
        } => {
            let by = format!(
                "{}@{}",
                std::env::var("USER").unwrap_or_else(|_| "someone".into()),
                profiles::checks::hostname()
            );
            match s.set_aside(&task, &at, hash.as_deref(), &reason, &by) {
                Ok(aside) => {
                    if json {
                        writeln!(out, "{}", serde_json::to_string(&aside)?)?;
                    } else {
                        writeln!(
                            out,
                            "set aside {} {} recorded at {} on {} {}: {}",
                            aside.task,
                            &aside.hash[..aside.hash.len().min(12)],
                            aside.run_at,
                            aside.branch,
                            aside.combination.short(),
                            aside.reason
                        )?;
                        writeln!(
                            out,
                            "no gate reads it from now; its line stays in the results"
                        )?;
                    }
                    Ok(0)
                }
                Err(e) => {
                    told::tell(
                        out,
                        json,
                        Kind::Refused,
                        &format!("refused: {e:#}"),
                        &["sluice status --all".to_string()],
                    )?;
                    Ok(2)
                }
            }
        }
        Command::Exec {
            branch,
            command,
            worktree,
            json,
            release,
        } => exec::Exec {
            branch,
            worktree,
            command,
            json,
            release,
            host,
            pushers,
            clock,
            pause,
        }
        .run(&s, out),
        Command::Equip { branch, at, json } => equip::Equip {
            branch,
            at,
            json,
            host,
            pushers,
        }
        .run(&s, out),
        Command::Preview {
            branch,
            revision,
            wait,
            json,
        } => preview::Ask {
            branch,
            revision,
            wait,
            json,
            host,
            clock,
            pause,
        }
        .run(&s, out),
        Command::Previews { json } => preview::list(&s, host, json, out),
        Command::Release {
            name: Some(name),
            accept_failure,
            json,
            ..
        } if name.starts_with(CHECKOUT) => release_checkout(&s, &name, accept_failure, json, out),
        Command::Release {
            name: Some(name),
            accept_failure,
            json,
            ..
        } => preview::release(&s, host, &name, accept_failure, json, out),
        Command::Release {
            name: None,
            worktree,
            accept_failure,
            json,
        } => {
            let released = exec::release(&s, host, &worktree, accept_failure, json, out)?;
            if json {
                let mut doc = serde_json::json!({
                    "worktree": released.worktree,
                    "released": released.released,
                });
                if accept_failure {
                    doc["accepted"] = released.accepted.into();
                }
                if let Some(f) = &released.failed {
                    doc["failed"] = f.clone().into();
                }
                writeln!(out, "{doc}")?;
            }
            Ok(if released.failed.is_some() {
                FAILED
            } else {
                LANDED
            })
        }
        Command::Supervise { dir } => {
            runs::supervise(&dir)?;
            Ok(0)
        }
        Command::Protect { remove, json } => {
            let dir = std::env::current_dir()?;
            if remove {
                let removed = protect::remove(&dir)?;
                if json {
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({ "protection": "removed", "changed": removed })
                    )?;
                } else {
                    writeln!(out, "branch protection removed")?;
                }
            } else {
                let installed =
                    protect::install(&dir, &std::fs::canonicalize(std::env::current_exe()?)?)?;
                // The hook guards the branch; a worktree holding it is
                // only said, never moved (ADR 0024).
                let held = s.held();
                if json {
                    let held = match held {
                        Ok(held) => held,
                        Err(e) => {
                            telemetry::warn(&format!(
                                "warning: the worktrees could not be read: {e:#}"
                            ));
                            Vec::new()
                        }
                    };
                    let held: Vec<serde_json::Value> = held
                        .iter()
                        .map(|h| serde_json::json!({ "worktree": h.worktree, "branch": h.branch }))
                        .collect();
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({
                            "protection": "installed", "changed": installed, "held": held,
                        })
                    )?;
                    return Ok(0);
                }
                writeln!(out, "branch protection installed")?;
                match held {
                    Ok(held) => write_held(&held, out)?,
                    Err(e) => writeln!(out, "warning: the worktrees could not be read: {e:#}")?,
                }
            }
            Ok(0)
        }
        Command::Branch { command } => branch::run(&s, command, out),
        Command::Forge {
            command: ForgeCommand::Check { branch, json },
        } => profiles::forge_check(
            &std::env::current_dir()?,
            branch.as_deref(),
            json,
            out,
            std::rc::Rc::new(retry::Policy::command()),
        ),
        Command::Credential { operation } => {
            identity::helper(&operation, &mut std::io::stdin().lock(), out)?;
            Ok(0)
        }
        Command::Hook { name, state } => match name.as_str() {
            "reference-transaction" => protect::hook(&s, host, &state, &mut std::io::stdin()),
            other => anyhow::bail!("sluice runs no {other} hook"),
        },
    }
}

/// 121: a branch held because the revision it is read at lacks its last
/// landing, with both revisions and the two ways out.
pub(crate) fn held_line(branch: &str, hold: &BranchHold) -> String {
    let (read, lacks) = (hold.read.short(), hold.lacks.short());
    format!(
        "held: {branch} reads {read}, which lacks the landing {lacks}; \
         put the landing back on {branch}, or run sluice branch accept {branch} {read}"
    )
}

/// 121: say `branch`'s hold once: when it begins, and again for another
/// revision read lacking the landing. `said` is what was said last.
fn tell_hold(
    out: &mut dyn Write,
    branch: &str,
    hold: Option<&BranchHold>,
    said: &mut Option<(Rev, Rev)>,
) -> Result<()> {
    let now = hold.map(|h| (h.read.clone(), h.lacks.clone()));
    if now != *said {
        if let Some(h) = hold {
            writeln!(out, "{}", held_line(branch, h))?;
        }
        *said = now;
    }
    Ok(())
}

/// 26, 121, rule core.14: say that `sluice run` passed `branch` by, its
/// status read or round failing with `words`, once while those words
/// stand: when it begins, and again for other words. `said` holds the
/// words each branch was last passed by with; a branch that reads leaves
/// it, so a refusal after a pass that read it is said again.
fn tell_passed(
    out: &mut dyn Write,
    branch: &str,
    words: &str,
    said: &mut BTreeMap<String, String>,
) -> Result<()> {
    if said.get(branch).map(String::as_str) != Some(words) {
        writeln!(out, "{branch} could not be read, passed by: {words}")?;
        said.insert(branch.to_string(), words.to_string());
    }
    Ok(())
}

/// 121: the hold as `--json` carries it.
pub(crate) fn held_json(hold: &BranchHold) -> serde_json::Value {
    serde_json::json!({ "read": hold.read, "lacks": hold.lacks, "since": hold.since })
}

/// Each submission's name as a view's lines show it (26), by its number:
/// its subject, or the branch it was submitted from, on one line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names(BTreeMap<u64, String>);

impl Names {
    /// Each of `subs` by its own name.
    pub fn of<'a>(subs: impl IntoIterator<Item = &'a Submission>) -> Names {
        let mut names = Names::default();
        for s in subs {
            names.insert(s.id, s.name());
        }
        names
    }

    /// Names `id` by `name`, as a line shows it; a name that shows as
    /// nothing leaves `id` to its number.
    pub fn insert(&mut self, id: u64, name: &str) {
        let shown = sluice_core::text::shown(name);
        if !shown.is_empty() {
            self.0.insert(id, shown);
        }
    }

    pub fn get(&self, id: u64) -> Option<&str> {
        self.0.get(&id).map(String::as_str)
    }
}

/// Each of `ids` as one line names it (26): `#12 Batch review cards` the
/// first time the line shows it, where `names` has a name for it, and
/// `#12` after, or with no name; `said` holds what the line has named.
pub(crate) fn named(ids: &[u64], names: Option<&Names>, said: &mut BTreeSet<u64>) -> Vec<String> {
    ids.iter()
        .map(|&id| match names.and_then(|n| n.get(id)) {
            Some(name) if said.insert(id) => format!("#{id} {name}"),
            _ => format!("#{id}"),
        })
        .collect()
}

/// `#12 Batch review cards, #13 unit/forecast`, or `#12, #13` with no
/// `names`.
pub(crate) fn hashes(ids: &[u64], names: Option<&Names>, said: &mut BTreeSet<u64>) -> String {
    named(ids, names, said).join(", ")
}

/// `bolt/x + #12 Batch review cards + #13 unit/forecast`, or `bolt/x + #12
/// + #13` with no `names`.
fn combination(
    branch: &str,
    ids: &[u64],
    names: Option<&Names>,
    said: &mut BTreeSet<u64>,
) -> String {
    let mut s = branch.to_string();
    for n in named(ids, names, said) {
        s.push_str(&format!(" + {n}"));
    }
    s
}

/// A branch's revision could not be read: on its remote, when its policy
/// names one, which is where it must be made.
pub(crate) fn no_branch(branch: &str, pushers: &dyn Pushers) -> Result<String> {
    Ok(match pushers.pusher(branch)? {
        Some(p) => format!(
            "refused: {branch} is not on {r}; make it with sluice branch create {branch} --from main",
            r = p.remote
        ),
        None => format!("refused: no branch {branch}"),
    })
}

/// The branch the driver reads for `worktree`, by which a way on names
/// the agent working there; the worktree as given when it reads none.
fn worktree_name(s: &Sluice<'_>, worktree: &str) -> String {
    match s.seams().driver.session(worktree) {
        Ok(x) if !x.from.worktree.is_empty() => x.from.worktree,
        _ => worktree.to_string(),
    }
}

/// What a refusal names `head` by: a place as [`worktree_name`] names
/// it; a branch named as it was given, since no worktree is read for it
/// (124; rule core.48).
fn head_name(s: &Sluice<'_>, head: &SubmitHead) -> String {
    match head {
        SubmitHead::Worktree(w) => worktree_name(s, w),
        SubmitHead::Branch(b) => b.clone(),
    }
}

/// `named` is `worktree` as [`worktree_name`] names it.
fn refusal(
    r: &SubmitRefusal,
    worktree: &str,
    named: &str,
    branch: &str,
    pushers: &dyn Pushers,
) -> Result<String> {
    Ok(match r {
        SubmitRefusal::Dirty(paths) => {
            let mut s = format!("refused: {worktree} holds changes beyond its commits:");
            for p in paths {
                s.push_str(&format!("\n  {p}"));
            }
            s.push_str(&format!(
                "\n{} commits or removes them and submits again",
                words::agent_in(named)
            ));
            s
        }
        SubmitRefusal::Detached => {
            format!(
                "refused: {worktree} is detached: the agent working in it submits from a branch"
            )
        }
        SubmitRefusal::NoBranch => return no_branch(branch, pushers),
        SubmitRefusal::NotManaged => {
            format!("refused: no [branches] table in .config/sluice.toml on {branch} names it")
        }
        SubmitRefusal::UnknownAfter(ids) => {
            format!(
                "refused: --after names no submission to {branch}: {}",
                hashes(ids, None, &mut BTreeSet::new())
            )
        }
        SubmitRefusal::NoHead(from) => format!("refused: no branch {from} to submit"),
        SubmitRefusal::ToItself => format!("refused: {worktree} is not submitted to itself"),
        SubmitRefusal::NotLandedOn => format!(
            "refused: {worktree} is no branch sluice lands on: a branch of a worktree's own is \
             submitted from that worktree"
        ),
        SubmitRefusal::NamedTwice(earlier) if earlier == worktree => {
            format!("refused: {worktree} is named twice")
        }
        SubmitRefusal::NamedTwice(earlier) => {
            format!("refused: {worktree} is {earlier}'s worktree, named twice")
        }
        SubmitRefusal::KeptOut(kept) => words::kept_out_refusal(branch, named, kept),
    })
}

/// The commands that move a refused submit on.
fn refusal_next(r: &SubmitRefusal, branch: &str) -> Vec<String> {
    match r {
        SubmitRefusal::Dirty(_) => vec![format!("sluice submit {branch}")],
        SubmitRefusal::Detached => vec![],
        SubmitRefusal::NoBranch => vec![
            format!("sluice branch create {branch} --from main"),
            "sluice overview".into(),
        ],
        SubmitRefusal::NotManaged => vec!["sluice overview".into()],
        SubmitRefusal::UnknownAfter(_) => vec![format!("sluice status {branch}")],
        SubmitRefusal::NoHead(_) => vec!["sluice overview".into()],
        SubmitRefusal::ToItself => vec![],
        SubmitRefusal::NotLandedOn => vec!["sluice overview".into()],
        SubmitRefusal::NamedTwice(_) => vec![],
        SubmitRefusal::KeptOut(_) => vec![format!("sluice status {branch}")],
    }
}

/// 64, 124, 129: `sluice submit --check`: formed on the branch and its
/// queue as the host's live reader last read them, asking the remote
/// nothing, where that reader keeps its tick and its read holds all the
/// forecast forms from; otherwise read as a round reads them.
#[allow(clippy::too_many_arguments)]
fn forecast(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    worktree: &str,
    from: Option<&str>,
    branch: &str,
    after: &[u64],
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let got = match forecast_read(s, host, worktree, from, branch, after)? {
        Some(got) => got,
        None => match from {
            Some(from) => s.forecast_branch(from, branch, after)?,
            None => s.forecast(worktree, branch, after)?,
        },
    };
    let f = match got {
        Ok(f) => f,
        Err(r) => {
            let text = match &r {
                SubmitRefusal::NoHead(from) => format!("refused: no branch {from} to forecast"),
                _ => refusal(&r, worktree, &worktree_name(s, worktree), branch, pushers)?,
            };
            told::tell(out, json, Kind::Refused, &text, &refusal_next(&r, branch))?;
            return Ok(CONFLICT);
        }
    };
    if json {
        writeln!(out, "{}", forecast_json(&f))?;
    } else {
        writeln!(out, "{}", forecast_text(&f.forecast, branch))?;
    }
    Ok(match f.forecast {
        Forecast::Clear => LANDED,
        Forecast::Conflict { .. } => CONFLICT,
    })
}

/// 64, 129: the forecast formed on the read of `branch` in the document of
/// the host's reader, while that reader keeps its tick (rule core.25):
/// `--head`'s branch taken from the same read where it holds it, and read
/// as today, once, where not. `None` where there is no such reader, its
/// read does not hold the branch, or holds too little to form on.
fn forecast_read(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    worktree: &str,
    from: Option<&str>,
    branch: &str,
    after: &[u64],
) -> Result<Option<Result<Forecasted, SubmitRefusal>>> {
    let now = std::time::SystemTime::now();
    // A reader this cannot look for is none: the forecast reads as a round.
    let found = host
        .readers()
        .and_then(|dir| reader::current(&dir, &host.repository()?, now));
    let Ok(Some(doc)) = found else {
        return Ok(None);
    };
    let Some(view) = branch_in(&doc, branch) else {
        return Ok(None);
    };
    let Some(from) = from else {
        let session = s.seams().driver.session(worktree)?;
        if session.detached {
            return Ok(None);
        }
        return s.forecast_read(&view, &session.head, &session.from.worktree, after);
    };
    if let Some(head) = branch_in(&doc, from).and_then(|v| v.revision) {
        return s.forecast_read(&view, &head, from, after);
    }
    let Some(head) = s.seams().branch.read(from)? else {
        return Ok(Some(Err(SubmitRefusal::NoHead(from.into()))));
    };
    Ok(Some(match s.forecast_read(&view, &head, from, after)? {
        Some(got) => got,
        None => s.forecast_head(&head, from, branch, after)?,
    }))
}

/// A forecast as `submit --check --json` and `exec --json` print it: its
/// conflict, or null, and what it was formed from (64, 65).
pub(crate) fn forecast_json(f: &Forecasted) -> serde_json::Value {
    let conflict = match &f.forecast {
        Forecast::Clear => serde_json::Value::Null,
        Forecast::Conflict { paths, with } => serde_json::json!({ "paths": paths, "with": with }),
    };
    serde_json::json!({
        "conflict": conflict,
        "head": f.head,
        "onto": f.onto,
        "ahead": f.ahead,
    })
}

pub(crate) fn forecast_text(f: &Forecast, branch: &str) -> String {
    match f {
        Forecast::Clear => format!("no conflict with {branch} or anything queued"),
        Forecast::Conflict { paths, with } => {
            let w: Vec<String> = with
                .iter()
                .map(|c| format!("#{} ({}, {})", c.id, c.from.agent, c.from.worktree))
                .collect();
            let with = if w.is_empty() {
                branch.to_string()
            } else {
                w.join(", ")
            };
            format!("would conflict: {} with {with}", paths.join(", "))
        }
    }
}

/// `sluice submit` to a branch with no pull requests: the subject, where
/// given, names the work its landing commit lands (23).
#[allow(clippy::too_many_arguments)]
fn submit(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    req: SubmitRequest,
    no_wait: bool,
    json: bool,
    confirms: Confirms,
    pause: &dyn Pause,
    tick: Duration,
    out: &mut dyn Write,
) -> Result<i32> {
    let accepted = match s.submit(&req)? {
        Ok(a) => a,
        Err(r) => {
            let named = head_name(s, &req.head);
            let text = refusal(&r, req.head.name(), &named, &req.branch, pushers)?;
            let next = refusal_next(&r, &req.branch);
            told::tell(out, json, Kind::Refused, &text, &next)?;
            return Ok(CONFLICT);
        }
    };
    let id = accepted.id;
    let copy = accepted.copy.as_ref();
    if json && no_wait {
        let mut doc =
            serde_json::json!({ "id": id, "branch": req.branch, "ahead": accepted.ahead });
        // ADR 0050: only a submission taken unchecked has any.
        if !accepted.warnings.is_empty() {
            doc["warnings"] = serde_json::json!(accepted.warnings);
        }
        writeln!(out, "{}", with_copy(doc, copy)?)?;
        return Ok(0);
    }
    if !json {
        let ahead = queued_ahead(&accepted.ahead, |i| format!("#{i}"));
        // 124, D9: a branch named is said by its name.
        let from = match &req.head {
            SubmitHead::Worktree(_) => String::new(),
            SubmitHead::Branch(b) => format!(" from {b}"),
        };
        writeln!(out, "submission {id} to {}{from} · {ahead}", req.branch)?;
        if let Some(copy) = copy {
            writeln!(out, "{}", copy_differs_line(copy))?;
        }
        for w in &accepted.warnings {
            writeln!(out, "warning: {w}")?;
        }
    }
    if no_wait {
        return Ok(0);
    }
    // Keep the queue moving while waiting (walkthrough section 6). Its
    // own submission is read on its target alone (record 0114).
    let sub = s
        .state_on(&req.branch, id)?
        .expect("a submission just made");
    let waiting = Waiting {
        host,
        clock,
        pulls: None,
        follows: pulls,
        deadline: None,
        json,
        pause,
        tick,
        copy,
        // 124: a branch named is delivered by nothing (record 0090).
        worktree: match &req.head {
            SubmitHead::Worktree(w) => Some(w),
            SubmitHead::Branch(_) => None,
        },
        confirms,
        named: None,
    };
    await_submission(s, &waiting, sub, Some(State::Queued), out)
}

/// What a submission was queued behind, as a submit says it, each named
/// by `name`.
fn queued_ahead(ahead: &[u64], name: impl Fn(u64) -> String) -> String {
    match ahead.len() {
        0 => "nothing queued ahead".to_string(),
        n => {
            let named: Vec<String> = ahead.iter().map(|i| name(*i)).collect();
            format!("{n} queued ahead ({})", named.join(", "))
        }
    }
}

/// 125: `sluice submit` of several worktrees as one act: every head in the
/// order given, all queued or none. Waiting, it waits on each in that
/// order, which runs the rounds that decide the rest (rule core.4), then
/// reports each one's outcome in order, and exits with the first, in
/// that order, that did not land. Under `--json`, one document however it
/// ends.
#[allow(clippy::too_many_arguments)]
fn submit_group(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    req: GroupRequest,
    no_wait: bool,
    json: bool,
    pause: &dyn Pause,
    tick: Duration,
    out: &mut dyn Write,
) -> Result<i32> {
    let branch = &req.branch;
    let accepted = match s.submit_group(&req)? {
        Ok(a) => a,
        Err(refused) => return refused_group(s, &req, &refused, pushers, json, out),
    };
    let names = req.names();
    let items: Vec<(&String, Accepted)> = names.iter().zip(accepted).collect();
    if json && no_wait {
        let submissions: Vec<serde_json::Value> = items
            .iter()
            .map(|(w, a)| {
                let mut doc = serde_json::json!({ "id": a.id, "worktree": w, "ahead": a.ahead });
                // ADR 0050: only a submission taken unchecked has any.
                if !a.warnings.is_empty() {
                    doc["warnings"] = serde_json::json!(a.warnings);
                }
                with_copy(doc, a.copy.as_ref())
            })
            .collect::<Result<_>>()?;
        writeln!(
            out,
            "{}",
            serde_json::json!({ "branch": branch, "submissions": submissions })
        )?;
        return Ok(0);
    }
    if !json {
        for (w, a) in &items {
            let ahead = queued_ahead(&a.ahead, |i| format!("#{i}"));
            writeln!(out, "submission {} to {branch} from {w} · {ahead}", a.id)?;
            if let Some(copy) = &a.copy {
                writeln!(out, "{}", copy_differs_line(copy))?;
            }
            for warning in &a.warnings {
                writeln!(out, "warning: {warning}")?;
            }
        }
    }
    if no_wait {
        return Ok(0);
    }
    // Each record carries its own copy, below.
    let waiting = Waiting {
        host,
        clock,
        pulls: None,
        follows: pulls,
        deadline: None,
        json,
        pause,
        tick,
        copy: None,
        worktree: None,
        confirms: Confirms::No,
        named: None,
    };
    // Each in turn; once stopped, the rest as they stand.
    let mut subs = Vec::new();
    // Record 0100 D3: what carried each one's work, where its wait
    // followed one.
    let mut carriers = Vec::new();
    let mut stopped = None;
    // 122: what failed after a landing, said once the outcomes are.
    let mut after = Vec::new();
    for (_, a) in &items {
        let sub = s.state_on(branch, a.id)?.expect("a submission just made");
        if stopped.is_some() {
            subs.push(sub);
            carriers.push(None);
            continue;
        }
        match wait_on(s, &waiting, sub, Some(State::Queued), out)? {
            Waited::Decided(sub, by, e) => {
                after.extend(e);
                subs.push(sub);
                carriers.push(by);
            }
            // A group of worktrees follows no pull request.
            Waited::Request(sub, _) => {
                subs.push(sub);
                carriers.push(None);
            }
            Waited::Undecided(sub, by, code, _) => {
                stopped = Some(code);
                subs.push(sub);
                carriers.push(by);
            }
            Waited::Unread(sub, e) => {
                // Said on stderr; the act's answer holds it as it stands.
                let (what, again) = (format!("#{}", sub.id), format!("sluice wait {}", sub.id));
                let code = could_not_read(&e, &what, &again, false, &mut std::io::sink())?;
                stopped = Some(code);
                subs.push(sub);
                carriers.push(None);
            }
        }
    }
    if json {
        let mut submissions = Vec::new();
        for (((w, a), sub), by) in items.iter().zip(&subs).zip(&carriers) {
            let mut record = with_carried(serde_json::to_value(sub)?, by.as_ref());
            record["worktree"] = serde_json::json!(w);
            submissions.push(with_copy(record, a.copy.as_ref())?);
        }
        let doc = serde_json::json!({ "branch": branch, "submissions": submissions });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        for (((w, _), sub), by) in items.iter().zip(&subs).zip(&carriers) {
            writeln!(out, "submission {} from {w}:", sub.id)?;
            if sub.state.is_out() {
                report(
                    s,
                    sub,
                    branch,
                    &BTreeMap::new(),
                    by.as_ref(),
                    None,
                    None,
                    false,
                    out,
                )?;
            } else {
                writeln!(out, "still waiting: sluice wait {}", sub.id)?;
            }
        }
    }
    for e in &after {
        after_landing(e);
    }
    if let Some(code) = stopped {
        return Ok(code);
    }
    Ok(subs
        .iter()
        .map(exit_for)
        .find(|c| *c != LANDED)
        .unwrap_or(LANDED))
}

/// A refused act: each refused worktree with why, said once each, and
/// nothing submitted.
fn refused_group(
    s: &Sluice<'_>,
    req: &GroupRequest,
    refused: &[(String, SubmitRefusal)],
    pushers: &dyn Pushers,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let branch = &req.branch;
    let again = group_again(req);
    let (mut texts, mut next, mut worktrees) = (Vec::new(), Vec::new(), Vec::new());
    for (w, r) in refused {
        let named = match req.heads.iter().find(|h| h.name() == w) {
            Some(h) => head_name(s, h),
            None => worktree_name(s, w),
        };
        let text = refusal(r, w, &named, branch, pushers)?;
        if !texts.contains(&text) {
            texts.push(text);
        }
        for n in refusal_next(r, branch) {
            let n = if n == format!("sluice submit {branch}") {
                again.clone()
            } else {
                n
            };
            if !next.contains(&n) {
                next.push(n);
            }
        }
        if !worktrees.contains(w) {
            worktrees.push(w.clone());
        }
    }
    if json {
        let message: Vec<&str> = texts
            .iter()
            .map(|t| t.strip_prefix("refused: ").unwrap_or(t))
            .collect();
        let mut doc = told::document(Kind::Refused, &message.join("\n"), &next);
        doc["error"]["worktrees"] = serde_json::json!(worktrees);
        writeln!(out, "{doc}")?;
    } else {
        for t in &texts {
            writeln!(out, "{t}")?;
        }
        writeln!(
            out,
            "nothing submitted: the {} worktrees are submitted together or not at all",
            req.heads.len()
        )?;
    }
    Ok(CONFLICT)
}

/// The act submitted again: `sluice submit <branch> --worktree …`.
fn group_again(req: &GroupRequest) -> String {
    format!(
        "sluice submit {} {}",
        req.branch,
        req.heads
            .iter()
            .map(|h| match h {
                SubmitHead::Worktree(w) => format!("--worktree {w}"),
                SubmitHead::Branch(b) => format!("--head {b}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// 125: an act under pull requests refused for its pull requests'
/// conditions: what it pushed and opened, each pull request short of its
/// conditions with what it lacks, and that every one of the act is a
/// draft at its head, which the same act takes once each meets them.
fn short_group(
    req: &GroupRequest,
    opened: &[Opened],
    short: &[Short],
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let again = group_again(req);
    let needs: Vec<String> = short
        .iter()
        .map(|x| {
            format!(
                "#{} from {} needs {}",
                x.number,
                x.worktree,
                x.lacks.join(", ")
            )
        })
        .collect();
    if json {
        let mut doc = told::document(Kind::Refused, &needs.join("\n"), &[again]);
        let worktrees: Vec<&str> = short.iter().map(|x| x.worktree.as_str()).collect();
        doc["error"]["worktrees"] = serde_json::json!(worktrees);
        writeln!(out, "{doc}")?;
    } else {
        for o in opened {
            writeln!(out, "{}", o.said)?;
        }
        for n in &needs {
            writeln!(out, "refused: {n}")?;
        }
        let numbers: Vec<u64> = opened.iter().map(|o| o.number).collect();
        writeln!(
            out,
            "nothing submitted: {} are drafts at their heads",
            hashes(&numbers, None, &mut BTreeSet::new())
        )?;
        writeln!(
            out,
            "once each meets its conditions, the same submit takes them all: {again}"
        )?;
    }
    Ok(CONFLICT)
}

/// 124: a submit's `--json` document, carrying `copy` where the host's
/// copy differs from the remote's revision it submitted.
fn with_copy(mut doc: serde_json::Value, copy: Option<&CopyDiffers>) -> Result<serde_json::Value> {
    if let Some(copy) = copy {
        doc["copy"] = serde_json::to_value(copy)?;
    }
    Ok(doc)
}

/// A submission's record as a `--json` document, with `copy` as
/// [`with_copy`] adds it.
fn record_json(sub: &Submission, copy: Option<&CopyDiffers>) -> Result<String> {
    Ok(match copy {
        Some(_) => serde_json::to_string_pretty(&with_copy(serde_json::to_value(sub)?, copy)?)?,
        None => serde_json::to_string_pretty(sub)?,
    })
}

/// How a command waits on a submission.
struct Waiting<'a> {
    /// Where the repository's reader on the host keeps its files, and
    /// what the overview it writes reads of the host (129).
    host: &'a dyn exec::Host,
    clock: &'a dyn Clock,
    /// On a branch that takes pull requests: taken in before each round,
    /// as a waiting pull-request submit does.
    pulls: Option<&'a dyn PullRequests>,
    /// 137, design D5: the pull requests the host follows, which the
    /// reader's document the wait writes carries as the host kept them.
    follows: &'a dyn PullRequests,
    /// When `sluice wait --timeout` runs out.
    deadline: Option<chrono::DateTime<chrono::Utc>>,
    json: bool,
    pause: &'a dyn Pause,
    /// How long `pause` pauses, the tick the wait names as its reader
    /// (64, 129).
    tick: Duration,
    /// 124: how the host's copy differed from what the submit waited on
    /// submitted, carried by its `--json` document.
    copy: Option<&'a CopyDiffers>,
    /// 51: the worktree that waits, whose own outcome the wait hands over
    /// when it reports it; none for a wait that hands over nothing.
    worktree: Option<&'a str>,
    /// 51: whether its hand-over confirms on receipt (design D5).
    confirms: Confirms,
    /// The branch `sluice wait <n> --branch` was given, which every
    /// command the wait suggests for its submission names (record 0114);
    /// none for any other wait.
    named: Option<&'a str>,
}

/// 51: whether a wait's hand-over of the outcome it reports confirms it on
/// receipt: never, unless its caller says so, for any outcome or, for
/// `wt submit`, for a landing whose worktree it removes (weir 103; design
/// D5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirms {
    No,
    Every,
    Landing,
}

impl Confirms {
    fn of(confirm: bool, landing: bool) -> Confirms {
        match (confirm, landing) {
            (true, _) => Confirms::Every,
            (false, true) => Confirms::Landing,
            (false, false) => Confirms::No,
        }
    }

    /// Whether the hand-over of `sub`'s outcome confirms it.
    fn confirms(self, sub: &Submission) -> bool {
        match self {
            Confirms::No => false,
            Confirms::Every => true,
            Confirms::Landing => sub.state == State::Landed,
        }
    }
}

/// ADR 0041: wait on `sub` to its outcome, or as `waiting` bounds it, and
/// say it: the loop under a waiting submit and `sluice wait`. `said` is
/// the state its caller has said already. Under `--json`, one document
/// however it ends: the submission's record, with the copy's `copy` and,
/// where the wait followed what carried its work, `carried_by` (record
/// 0100 D3). 51: an outcome the waiting worktree's own and pending is
/// marked delivered to it once the report is written, and the record
/// reported carries that mark; an undecided end marks nothing.
fn await_submission(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    sub: Submission,
    said: Option<State>,
    out: &mut dyn Write,
) -> Result<i32> {
    let (sub, by, code, tested) = match wait_on(s, waiting, sub, said, out)? {
        Waited::Decided(sub, by, after) => {
            let branch = sub.branch.clone();
            let names = BTreeMap::new();
            let code = report_delivering(
                s,
                waiting.worktree,
                sub,
                &branch,
                &names,
                by.as_ref(),
                waiting.copy,
                waiting.json,
                waiting.confirms,
                out,
            )?;
            if let Some(e) = after {
                after_landing(&e);
            }
            return Ok(code);
        }
        Waited::Unread(sub, e) => {
            let again = asked("sluice wait", sub.id, waiting.named);
            return could_not_read(&e, &format!("#{}", sub.id), &again, waiting.json, out);
        }
        Waited::Request(sub, number) => {
            let Some(pulls) = waiting.pulls else {
                anyhow::bail!(
                    "#{} names pull request #{number}, and its branch takes none",
                    sub.id
                );
            };
            let mode = PullMode::Follow {
                number,
                own: &sub,
                deadline: waiting.deadline,
            };
            let worktree = SubmitHead::Worktree(waiting.worktree.unwrap_or(".").to_string());
            return submit_pull(
                s,
                waiting.host,
                pulls,
                waiting.clock,
                &worktree,
                &sub.branch,
                mode,
                waiting.json,
                waiting.confirms,
                waiting.pause,
                waiting.tick,
                out,
            );
        }
        Waited::Undecided(sub, by, code, tested) => (sub, by, code, tested),
    };
    if waiting.json {
        let doc = with_copy(serde_json::to_value(&sub)?, waiting.copy)?;
        let mut doc = with_carried(doc, by.as_ref());
        // 84, record 0074; 114, record 0105: what its combination waits
        // for and why its last run had no outcome, as the view the wait
        // followed has it (rule core.25).
        if let Some(c) = &tested {
            under_test(&mut doc, c);
        }
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        let again = asked("sluice wait", sub.id, waiting.named);
        writeln!(out, "still waiting: {again}")?;
    }
    Ok(code)
}

/// 51, D8: a submission's document, its record as it is, with, for one
/// landed, failed or conflicting, its `notice`: the words the round pushes
/// into its session and `sluice inbox` gives it, from `decided` where the
/// round's outbox item names its outcome, else from the record's own. A
/// withdrawn, queued or testing one has none. The one document `sluice
/// inbox --json`, `sluice status <branch> [--all] --json` and `sluice
/// status <n> --json` give a submission: a contract a caller reads, which
/// only gains fields.
fn decided_document(sub: &Submission, decided: Option<&Decided>) -> Result<serde_json::Value> {
    let mut doc = serde_json::to_value(sub)?;
    let own = sub.decided();
    if let Some(d) = decided.or(own.as_ref()) {
        doc["notice"] = sluice_core::notice(sub.id, &sub.branch, d).into();
    }
    Ok(doc)
}

/// 84, record 0074; 114, record 0105: `doc`, `sub`'s record, with what its
/// combination under test waits for and why its last run had no outcome,
/// as [`under_test`] gives them. It decides nothing, so a read that fails
/// adds nothing.
fn with_waiting(s: &Sluice<'_>, sub: &Submission, mut doc: serde_json::Value) -> serde_json::Value {
    let view = s.branch_view_of(&sub.branch).ok();
    if let Some(c) = view.as_ref().and_then(|v| progress::own(v, sub.id)) {
        under_test(&mut doc, c);
    }
    doc
}

/// What `doc`, a submission's document, gives of `c`, its combination
/// under test: under `waiting`, the hosts that turned its run down each
/// with why, where any did (84, record 0074); under `lost`, each run whose
/// last run had no outcome, its host, its system where the policy names
/// one and each check it left not run with why, whatever the cause, until
/// that run's next result (114; record 0105, design D7 of the change
/// `a-pass-nothing-proves-blames-no-one`).
fn under_test(doc: &mut serde_json::Value, c: &CandidateView) {
    if !c.waiting.is_empty() {
        doc["waiting"] = serde_json::json!(c.waiting);
    }
    if !c.lost.is_empty() {
        doc["lost"] = serde_json::json!(c.lost);
    }
}

/// 51: report `sub`, decided, as [`report`] does, its document giving its
/// `delivery`; when it is owed to the waiting `worktree`'s reader, it is
/// recorded handed over by `wait` once the report is written and flushed,
/// confirmed with it where `confirms` says, and the document gives it as
/// that leaves it. A hand-over that fails to be recorded is said and leaves
/// the outcome owed for `sluice inbox`. Only `sub` is handed over, the
/// submission the wait reports as its own, never `by`, what carried its
/// work, whose outcome stays owed for its own carriers (record 0100 D3;
/// design D5, D13).
#[allow(clippy::too_many_arguments)]
fn report_delivering(
    s: &Sluice<'_>,
    worktree: Option<&str>,
    sub: Submission,
    branch: &str,
    names: &BTreeMap<u64, String>,
    by: Option<&CarriedBy>,
    copy: Option<&CopyDiffers>,
    json: bool,
    confirms: Confirms,
    out: &mut dyn Write,
) -> Result<i32> {
    let delivering = worktree.and_then(|w| delivering(s, w, &sub));
    let confirm = confirms.confirms(&sub);
    let delivery = match &delivering {
        Some((owed, _)) => owed
            .outcomes
            .first()
            .map(|o| o.left("wait", &owed.at, confirm)),
        None if json => s.delivery_of(&sub).unwrap_or_else(|e| {
            telemetry::warn(&format!(
                "sluice: warning: could not read #{}'s delivery: {e:#}",
                sub.id
            ));
            None
        }),
        None => None,
    };
    let delivery = delivery.map(|d| d.to_json());
    let code = report(s, &sub, branch, names, by, copy, delivery, json, out)?;
    out.flush()?;
    if let Some((owed, to)) = delivering {
        if let Err(e) = owed.hand("wait", &to, confirm) {
            telemetry::error(&format!(
                "sluice: could not record #{}'s hand-over, so sluice inbox hands it over: {e:#}",
                sub.id
            ));
        }
    }
    Ok(code)
}

/// How a wait ended: always with the submission it waits on, its own,
/// and, where it followed what carried its work in its stead, that, as
/// it stands (record 0100 D1, D2).
enum Waited {
    /// Landed, failed, conflict or withdrawn; when it landed in a round
    /// that then failed, what failed after the landing (122).
    Decided(Submission, Option<CarriedBy>, Option<anyhow::Error>),
    /// As it stands when the wait's time ran out ([`WAITING`]) or it was
    /// stopped ([`STOPPED`]), with its combination under test, or that of
    /// what it follows, as last read: what it waits for (84, record 0074)
    /// and why its last run had no outcome (114, record 0105).
    Undecided(Submission, Option<CarriedBy>, i32, Option<CandidateView>),
    /// As last read, when a round failed as the forge could not answer
    /// for the retry policy's time.
    Unread(Submission, anyhow::Error),
    /// Withdrawn, while pull request `n`, on a head that carries its work,
    /// is still to be taken in or is taken in on another host: the wait
    /// follows the request (67, 137).
    Request(Submission, u64),
}

/// What a wait does next with the submission it read decided, its own or
/// what it follows.
enum Next {
    /// End, as this says.
    End(Waited),
    /// Follow this submission, which carries its work, in its stead.
    Follow(Submission),
}

/// Record 0100 D2: what a wait on `own` does with `read`, decided: `own`
/// itself, or the submission it follows. What carries `read`'s work on is
/// followed; otherwise the wait ends with `own`, as decided where `read`
/// is `own`, and as [`own_end`] has it where `read` carried its work.
fn decided(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    own: &Submission,
    read: Submission,
    out: &mut dyn Write,
) -> Result<Next> {
    Ok(match carried(s, waiting, read, out)? {
        WorkGoes::Entry(e) => Next::Follow(e),
        WorkGoes::Request(n) => Next::End(Waited::Request(own.clone(), n)),
        // Landed by what carried its work: named, where `read` is its own,
        // and its own's end where `read` is what it followed.
        WorkGoes::Landed(o, e) => {
            let by = CarriedBy::Entry(e);
            let end = match o.id == own.id {
                true => o,
                false => own_end(s, own.clone(), Some(&by))?,
            };
            Next::End(Waited::Decided(end, Some(by), None))
        }
        WorkGoes::Outcome(o, _) if o.id == own.id => Next::End(Waited::Decided(o, None, None)),
        // What carried its work ended as its request closed: the request.
        WorkGoes::Outcome(_, Some(n)) => {
            let by = CarriedBy::Request(n, RequestEnd::Closed);
            let end = own_end(s, own.clone(), Some(&by))?;
            Next::End(Waited::Decided(end, Some(by), None))
        }
        WorkGoes::Outcome(o, None) => {
            let by = CarriedBy::Entry(o);
            let end = own_end(s, own.clone(), Some(&by))?;
            Next::End(Waited::Decided(end, Some(by), None))
        }
    })
}

/// What a wait does with a submission it read decided.
enum WorkGoes {
    /// Its outcome, to report, with the number of its pull request where
    /// that was read closed, which ended its work (record 0100 D2).
    Outcome(Submission, Option<u64>),
    /// Landed by the landing of what carried its work, a later submission
    /// from its place: its record as that landing landed it, and that
    /// submission (record 0100 D2).
    Landed(Submission, Submission),
    /// A later submission from its place that carries its work, to follow
    /// in its stead (67).
    Entry(Submission),
    /// Its pull request, to follow (137).
    Request(u64),
}

/// 122, 67, 137: a submission withdrawn whose work goes on is never
/// reported withdrawn. Its branch holds its head: it landed, as the commit
/// that landed it, whichever landing that was; where a later submission
/// from its place that holds its head landed, by that one's landing,
/// which names it, however long ago it landed (record 0100 D2, D3; a
/// branch landing by fast-forward would otherwise give its own head as
/// the commit that landed it). A later submission from
/// its place holds its head: the wait follows that one. Its pull request
/// is open, no draft, on a head that holds its head: the wait follows the
/// request, taken in again here or on another host, or the submission it
/// was taken in as. Any other outcome, and a withdrawal no work goes on
/// from, is reported as it stands.
fn carried(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    sub: Submission,
    out: &mut dyn Write,
) -> Result<WorkGoes> {
    if sub.state != State::Withdrawn {
        return Ok(WorkGoes::Outcome(sub, None));
    }
    let branch = s.seams().branch;
    // What cannot be read leaves the outcome as the records hold it, and
    // is said.
    let unread = |what: &str, e: anyhow::Error| {
        telemetry::warn(&format!(
            "sluice: could not read whether #{}'s work went on, from {what}: {e:#}",
            sub.id
        ))
    };
    // A head that is this one's, or holds it.
    let carries =
        |head: &Rev| *head == sub.head || branch.contains(head, &sub.head).unwrap_or(false);
    let landed = branch.read(&sub.branch).and_then(|tip| match tip {
        Some(tip) => branch.landed_by(&tip, &sub.head),
        None => Ok(None),
    });
    match landed {
        Ok(Some(to)) => {
            let by = landed_carrier(s, &sub, &carries).unwrap_or_else(|e| {
                unread(&format!("{}'s records", sub.branch), e);
                None
            });
            let to = by.as_ref().and_then(|e| e.landed_as.clone()).unwrap_or(to);
            let landed = Submission {
                state: State::Landed,
                landed_as: Some(to),
                landed_elsewhere: true,
                ..sub
            };
            return Ok(match by {
                Some(e) => WorkGoes::Landed(landed, e),
                None => WorkGoes::Outcome(landed, None),
            });
        }
        Ok(None) => {}
        Err(e) => unread(&sub.branch, e),
    }
    // 137: a hand-over's withdrawal says the host it handed over to.
    let left = left(&sub);
    let follows = |what: String, out: &mut dyn Write| -> Result<()> {
        if !waiting.json {
            writeln!(
                out,
                "#{} {left}: {what} carries its work, so this wait follows it",
                sub.id
            )?;
        }
        Ok(())
    };
    let later = s
        .status(&sub.branch)?
        .submissions
        .into_iter()
        .rfind(|e| e.id > sub.id && e.from.worktree == sub.from.worktree && carries(&e.head));
    if let Some(e) = later {
        follows(format!("#{}", e.id), out)?;
        return Ok(WorkGoes::Entry(e));
    }
    let (Some(pulls), Some(n)) = (
        waiting.pulls,
        waiting.pulls.and_then(|p| p.request_of(&sub.from.worktree)),
    ) else {
        return Ok(WorkGoes::Outcome(sub, None));
    };
    let followed = match pulls.follow(s.seams(), waiting.clock, &sub.branch, n) {
        Ok(f) => f,
        Err(e) => {
            unread(&format!("pull request #{n}"), e);
            return Ok(WorkGoes::Outcome(sub, None));
        }
    };
    let closed = matches!(followed, Followed::Closed).then_some(n);
    // Its request open on the head it had carries on only where another
    // host takes it in: a withdrawal by its submitter, its planner or the
    // operator rests it there (120).
    let goes_on = match followed {
        Followed::Done(e, _)
        | Followed::Open {
            submission: Some(e),
            ..
        } => (e.id != sub.id && carries(&e.head)).then_some(Some(e)),
        Followed::Open {
            submission: None,
            draft: false,
            head: Some(h),
            ..
        } => {
            let moved_on = h != sub.head.0 && carries(&Rev::new(h));
            (moved_on || pulls.handed_over(&sub)).then_some(None)
        }
        Followed::Landed { .. } | Followed::SentBack { .. } => Some(None),
        Followed::Open { .. } | Followed::Closed => None,
    };
    Ok(match goes_on {
        Some(Some(e)) => {
            follows(format!("#{}", e.id), out)?;
            WorkGoes::Entry(e)
        }
        Some(None) => {
            follows(format!("its pull request #{n}"), out)?;
            WorkGoes::Request(n)
        }
        None => WorkGoes::Outcome(sub, closed),
    })
}

/// Record 0100 D2: the later submission from `sub`'s place, a worktree's or
/// a pull request's, that `carries` its head and landed, whose landing is
/// the one that took `sub`'s work: the first such in the branch's live
/// records, else its place's latest, which the finished index names once
/// it has left them (rule core.41). `None` where none landed, as where
/// another queue's landing took it (record 0085).
fn landed_carrier(
    s: &Sluice<'_>,
    sub: &Submission,
    carries: &dyn Fn(&Rev) -> bool,
) -> Result<Option<Submission>> {
    let place = &sub.from.worktree;
    let landed_it = |e: &Submission| {
        e.id > sub.id && e.from.worktree == *place && e.state == State::Landed && carries(&e.head)
    };
    let live = s.status(&sub.branch)?.submissions;
    if let Some(e) = live.into_iter().find(|e| landed_it(e)) {
        return Ok(Some(e));
    }
    let latest = s
        .latest_of(&sub.branch, std::slice::from_ref(place))?
        .remove(place);
    Ok(latest.filter(|e| landed_it(e)))
}

/// Whether `e` is a spent allowance of GitHub's calls the retry policy
/// waited out to its end, a wait's deadline: the wait's time is up (134).
fn waited_out(e: &anyhow::Error) -> bool {
    retry::limited(e).is_some() && e.downcast_ref::<retry::Spent>().is_some()
}

/// `e`, a wait's read or round that failed, when it failed as a forge
/// could not answer for as long as the retry policy tried, which ends the
/// wait as it could not read (134; rule core.29); any other failure is
/// the wait's own.
fn unread(e: anyhow::Error) -> Result<anyhow::Error> {
    if sluice_seams::unanswered(&e) {
        Ok(e)
    } else {
        Err(e)
    }
}

/// A wait that could not read `what` (`e`, the forge's last answer once
/// the retry policy stopped): said on stderr, on its own line beginning
/// `sluice: could not read`, with how long it was tried, and apart from
/// any outcome, which only the records give; exit [`INTERNAL`], with
/// `again` to wait once more. Under `--json`, the error document in place
/// of the record.
fn could_not_read(
    e: &anyhow::Error,
    what: &str,
    again: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let lasted = retry::lasted(e).as_secs() as i64;
    let text = format!(
        "could not read {what} for {}, so whether it landed is not known: {}",
        progress::duration(lasted),
        retry::unspent(e)
    );
    telemetry::error(&format!("sluice: {text}"));
    if json {
        told::tell(out, true, Kind::Internal, &text, &[again.to_string()])?;
    } else {
        writeln!(out, "could not read {what}: {again}")?;
    }
    Ok(INTERNAL)
}

/// Read `sub` again until it is decided, running a round between reads
/// as a waiting driver does (1; rule core.4; ADR 0011), until the
/// deadline or Ctrl-C. It reads before its first round, so a submission
/// already decided runs nothing, and takes its first pass before it looks
/// at its deadline, so a wait whose start used its time up still moves its
/// queue once. Where `sluice run` lives on the host, it
/// runs no round and takes no pull request in: it follows the driver's
/// document, looking again at each tick, and runs rounds from the tick
/// after the driver stops (135). Each call a round makes to a remote is
/// tried again under the retry policy for the wait's five minutes (134);
/// a round that still fails as the forge could not answer ends the wait as
/// it could not read. Without `--json`, it says the
/// submission's state, or its combination's progress, each time either
/// moves (ADR 0042). It reads them as [`Reading`] does (129). It waits on
/// `own` and ends with it, whatever it follows: where a later submission
/// carries `own`'s work, it follows that one, its state and its
/// combination's progress, and ends with `own` as [`decided`] has it
/// (record 0100 D1, D2).
fn wait_on(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    mut own: Submission,
    said_before: Option<State>,
    out: &mut dyn Write,
) -> Result<Waited> {
    let branch = own.branch.clone();
    type Moved = (State, Option<progress::Moved>);
    let mut said: Option<Moved> = said_before.map(|t| (t, None));
    // 121: the branch's hold as last said, so each is said once.
    let mut said_hold = None;
    let mut first = true;
    // Record 0100 D1: what carries `own`'s work, followed in its stead.
    let mut following: Option<Submission> = None;
    // Decided already: nothing read again, and no round run.
    if own.state.is_out() {
        match decided(s, waiting, &own, own.clone(), out)? {
            Next::End(w) => return Ok(w),
            Next::Follow(e) => following = Some(e),
        }
    }
    let mut reading = Reading::take(waiting.host)?;
    let _let_go = LetGo(waiting.pulls);
    reading.read(s, waiting, following.as_mut().unwrap_or(&mut own))?;
    loop {
        let sub = following.as_ref().unwrap_or(&own);
        if sub.state.is_out() {
            match decided(s, waiting, &own, sub.clone(), out)? {
                Next::End(w) => return Ok(w),
                Next::Follow(e) => {
                    let e = following.insert(e);
                    reading.read(s, waiting, e)?;
                    continue;
                }
            }
        }
        // As it stands, with what it follows as it stands.
        let undecided = |code: i32, reading: &Reading| {
            let by = following.clone().map(CarriedBy::Entry);
            let combination = reading.combination(following.as_ref().unwrap_or(&own).id);
            Waited::Undecided(own.clone(), by, code, combination)
        };
        let id = sub.id;
        if !waiting.json {
            // How far it has got decides nothing: a read that fails says
            // nothing this time.
            let view = reading.view.as_ref();
            let combination = view.and_then(|v| progress::own(v, id));
            let now: Moved = (sub.state, combination.map(progress::moved));
            if said.as_ref() != Some(&now) {
                match combination {
                    Some(c) => {
                        let line = progress::testing_line(&branch, c, waiting.clock.now(), None);
                        writeln!(out, "{line}")?;
                    }
                    None => match detail(&branch, sub, None, &mut BTreeSet::new()) {
                        d if d.is_empty() => writeln!(out, "#{id} {}", sub.state.word())?,
                        d => writeln!(out, "#{id} {} · {d}", sub.state.word())?,
                    },
                }
                said = Some(now);
            }
            if let Some(v) = view {
                tell_hold(out, &branch, v.held.as_ref(), &mut said_hold)?;
            }
        }
        // Its first pass is taken whatever its deadline, as a wait that
        // follows a pull request takes its first intake: a start that used
        // its time up still moves the queue once, and out of time it says
        // its submission as that pass left it (1).
        if !first {
            if waiting.deadline.is_some_and(|d| waiting.clock.now() >= d) {
                return Ok(undecided(WAITING, &reading));
            }
            if !waiting.pause.pause() {
                return Ok(undecided(STOPPED, &reading));
            }
        }
        first = false;
        // 129: what the last read kept by revision and this one does not
        // use is forgotten.
        s.next_pass();
        waiting.host.next_pass();
        let driven = reading.driven();
        if !driven {
            if let Some(pulls) = waiting.pulls {
                // A spent allowance waited out to the wait's deadline: the
                // wait's time is up; to Ctrl-C: stopped (134).
                let spent = intake(
                    pulls,
                    s.seams(),
                    waiting.clock,
                    &branch,
                    Taker::Wait,
                    waiting.json,
                    out,
                );
                if spent.is_some_and(|e| waited_out(&e)) {
                    let code = match INTERRUPTED.load(Ordering::SeqCst) {
                        true => STOPPED,
                        false => WAITING,
                    };
                    return Ok(undecided(code, &reading));
                }
            }
            // 137: another host takes the branch's pull requests in; this
            // wait runs no round for them.
            let away = waiting.pulls.and_then(|p| p.elsewhere(&branch));
            let round = match away {
                Some(_) => None,
                None => Some(s.round(&branch)),
            };
            match round {
                None => {}
                Some(Ok(round)) => {
                    exec::tell_abandoned(out, waiting.json, &round.released)?;
                    exec::tell_releases(out, waiting.json, &round.releases)?;
                }
                Some(Err(e)) => {
                    // Ctrl-C reaches the tools a round runs in the foreground.
                    if INTERRUPTED.load(Ordering::SeqCst) {
                        return Ok(undecided(STOPPED, &reading));
                    }
                    // Record 0100 D2: what it follows landed, its own end.
                    if let Some(landed) = landed(s, id, &branch) {
                        if landed.id == own.id {
                            return Ok(Waited::Decided(landed, None, Some(e)));
                        }
                        let by = CarriedBy::Entry(landed);
                        let end = own_end(s, own.clone(), Some(&by))?;
                        return Ok(Waited::Decided(end, Some(by), Some(e)));
                    }
                    return Ok(Waited::Unread(own.clone(), unread(e)?));
                }
            }
        }
        // A wait takes no lease, but lets go of the lock once a newer
        // watch has taken its, or once a driver lives; it follows from then
        // on.
        let drives = if driven {
            Drives::Another
        } else {
            Drives::Nobody
        };
        reading.reader.step(false, drives)?;
        reading.read(s, waiting, following.as_mut().unwrap_or(&mut own))?;
    }
}

/// How a wait reads its submission, its combination and its branch's
/// hold (129; ADR 0056; rule core.25): the first watch or wait of the
/// repository on the host is its reader, which reads the records and
/// writes the whole overview for the others, unless `sluice run` lives on
/// the host, which is the reader there (135); every other wait reads them
/// from the reader's document, and its own record only once a document no
/// longer holds it, as when it has left the queue. A wait that is the
/// reader lets go once a newer watch has taken its lease, and follows from
/// then on as any other wait.
struct Reading {
    reader: Reader,
    /// Where the host's readers and drivers keep their files, and the
    /// repository whose they are.
    readers: std::path::PathBuf,
    repository: String,
    /// Its branch's part of the overview, as last read.
    view: Option<BranchView>,
    /// The reader's last document that did not hold the submission: its
    /// record is read once for each such document, never at every pause.
    lacked: Option<serde_json::Value>,
}

impl Reading {
    /// Submission `id`'s combination under test, as last read: what it
    /// waits for (84, record 0074) and why its last run had no outcome
    /// (114, record 0105).
    fn combination(&self, id: u64) -> Option<CandidateView> {
        self.view
            .as_ref()
            .and_then(|v| progress::own(v, id))
            .cloned()
    }

    fn take(host: &dyn exec::Host) -> Result<Reading> {
        let (readers, repository) = (host.readers()?, host.repository()?);
        Ok(Reading {
            reader: Reader::take(&readers, &repository, host.version())?,
            readers,
            repository,
            view: None,
            lacked: None,
        })
    }

    /// Whether `sluice run` drives the repository on the host, as probed
    /// now: once a tick, the wait's round left to it (135).
    fn driven(&self) -> bool {
        reader::driver_live(&self.readers, &self.repository)
    }

    /// Read `sub` again, and its branch's part of the overview. A follower
    /// with no whole overview to read says nothing new.
    fn read(&mut self, s: &Sluice<'_>, waiting: &Waiting<'_>, sub: &mut Submission) -> Result<()> {
        let (id, branch) = (sub.id, sub.branch.clone());
        // The reader's document, when it was read from one.
        let followed = match &mut self.reader {
            Reader::Lead(lead) => {
                // A read whose start it could not mark leaves a forecast
                // reading past this reader, which decides nothing (64).
                let _ = lead.begin();
                let follows = Follows::Kept(waiting.follows);
                match overview::document(s, waiting.host, waiting.clock, follows) {
                    Ok((branches, doc)) => {
                        // Ctrl-C reaching the git a branch's read runs is no
                        // refusal of that branch's (26).
                        if !INTERRUPTED.load(Ordering::SeqCst) {
                            lead.write(&overview::ticked(doc, waiting.tick))?;
                        }
                        self.view = branches
                            .into_iter()
                            .find(|b| b.branch == branch && b.unread.is_none());
                        None
                    }
                    // The records, or the host's part, could not be read: the
                    // failure is the reader's document, in the words it was
                    // refused with (133), and its own part is read as a wait
                    // alone reads it. Ctrl-C reaching the git a read runs is no
                    // failure of the queues'.
                    Err(e) => {
                        if !INTERRUPTED.load(Ordering::SeqCst) {
                            let failed = told::document(Kind::Internal, &format!("{e:#}"), &[]);
                            lead.write(&overview::ticked(failed, waiting.tick))?;
                        }
                        self.view = s.branch_view_of(&branch).ok();
                        *sub = record(s, id, &branch)?;
                        return Ok(());
                    }
                }
            }
            Reader::Follow(follow) => {
                // The reader's read that failed is no whole document either:
                // the last view stands, and no record is read for it (129).
                let Some(doc) = follow.read().filter(|d| d["branches"].is_array()) else {
                    return Ok(());
                };
                self.view = branch_in(&doc, &branch);
                Some(doc)
            }
        };
        let held = self
            .view
            .as_ref()
            .and_then(|v| v.submissions.iter().find(|x| x.id == id));
        match (held, followed) {
            // In the queue: the overview's record is its record.
            (Some(x), _) => *sub = x.clone(),
            // Out of the queue, as the records just read say.
            (None, None) => *sub = record(s, id, &branch)?,
            // Out of the reader's queue, or not yet in it.
            (None, Some(doc)) if self.lacked.as_ref() != Some(&doc) => {
                *sub = record(s, id, &branch)?;
                self.lacked = Some(doc);
            }
            (None, Some(_)) => {}
        }
        Ok(())
    }
}

/// A probe of whether `sluice run` drives the host's repository, each
/// call a probe at that moment (135): the host's readers and repository
/// are read once, and a host that cannot say where they are reads as
/// having no driver, so a wait runs rounds rather than wait on nothing.
fn driven_by(host: &dyn exec::Host) -> impl Fn() -> bool {
    let at = host.readers().ok().zip(host.repository().ok());
    move || {
        at.as_ref()
            .is_some_and(|(dir, repository)| reader::driver_live(dir, repository))
    }
}

/// Submission `id`'s record, from the records of `branch` alone, whatever
/// other branch holds its number or cannot be read (26; record 0114).
fn record(s: &Sluice<'_>, id: u64, branch: &str) -> Result<Submission> {
    match s.lookup_on(branch, id)? {
        Lookup::Found(sub) => Ok(sub),
        Lookup::Unfound(unread) if unread.is_empty() => Err(anyhow::anyhow!(
            "submission {id} is no longer in {branch}'s records"
        )),
        Lookup::Unfound(unread) => {
            Err(anyhow::anyhow!("{}", unfound_lines(id, &unread).join("; ")))
        }
        Lookup::Several(several) => Err(anyhow::anyhow!(
            "{}",
            several_lines(id, "sluice wait", &several).join("; ")
        )),
    }
}

/// `branch`'s part of an overview document; `None` when it has none, or
/// when the reader could not read it (26).
fn branch_in(doc: &serde_json::Value, branch: &str) -> Option<BranchView> {
    let b = doc["branches"]
        .as_array()?
        .iter()
        .find(|b| b["branch"] == branch)?;
    serde_json::from_value::<BranchView>(b.clone())
        .ok()
        .filter(|b| b.unread.is_none())
}

/// 122: `branch`'s submission `id` as landed, when its work is on the
/// branch: its record, or the record the next round writes when the round
/// that moved the branch stopped before writing it (24). `None` when it is
/// not, or when the records cannot be read, which leaves the round's own
/// error the outcome. `branch` alone is read, whatever other branch holds
/// the number (26; record 0114).
fn landed(s: &Sluice<'_>, id: u64, branch: &str) -> Option<Submission> {
    let to = s.landed_as_on(branch, id).ok()??;
    let mut sub = s.state_on(branch, id).ok()??;
    sub.state = State::Landed;
    sub.landed_as = Some(to);
    Some(sub)
}

/// The latest submission to `branch` on `head`, as a pull request's is
/// taken in on the head its worktree pushed; `None` when there is none or
/// the records cannot be read.
fn taken_in(s: &Sluice<'_>, branch: &str, head: &sluice_seams::Rev) -> Option<u64> {
    let all = s.history(branch).ok()?;
    all.iter().rev().find(|x| x.head == *head).map(|x| x.id)
}

/// 122: what failed after a landing, said apart from its outcome, on
/// stderr and to telemetry as sluice failing itself is.
fn after_landing(e: &anyhow::Error) {
    telemetry::error(&format!("sluice: after the landing: {e:#}"));
}

/// A submission's outcome, as a waiting submit ends with it; `by`, what
/// carried its work, where the wait followed one in its stead (record 0100
/// D3).
#[allow(clippy::too_many_arguments)]
fn report(
    s: &Sluice<'_>,
    sub: &Submission,
    branch: &str,
    names: &BTreeMap<u64, String>,
    by: Option<&CarriedBy>,
    copy: Option<&CopyDiffers>,
    delivery: Option<serde_json::Value>,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let id = sub.id;
    let name = |i: u64| names.get(&i).cloned().unwrap_or_else(|| format!("#{i}"));
    let combination = |ids: &[u64]| {
        let mut s = branch.to_string();
        for i in ids {
            s.push_str(&format!(" + {}", name(*i)));
        }
        s
    };
    if json {
        let mut doc = with_carried(with_copy(serde_json::to_value(sub)?, copy)?, by);
        // 51, D13: a decided submission's delivery, where it has one.
        if let Some(delivery) = delivery {
            doc["delivery"] = delivery;
        }
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
        return Ok(exit_for(sub));
    }
    let log = sluice_core::line::Line::read_log(&sluice_seams::State::read(s_state(s), branch)?)?;
    if let Some(t) = &sub.tested {
        // Only its last test: the same head on the same revision is
        // the same combination, tested again on a resubmission.
        let last = log.iter().rposition(|l| {
            matches!(&l.event, Event::Testing { combination, .. } if *combination == t.combination)
        });
        let (mut ran, mut replayed) = (0, 0);
        for l in &log[last.unwrap_or(0)..] {
            match &l.event {
                Event::Ran { combination, .. } if *combination == t.combination => ran += 1,
                Event::Replayed { combination, .. } if *combination == t.combination => {
                    replayed += 1
                }
                _ => {}
            }
        }
        writeln!(
            out,
            "testing {} · {ran} tasks to run, {replayed} replayed",
            combination(&t.submissions)
        )?;
    }
    match sub.state {
        State::Landed => {
            let to = sub.landed_as.clone().expect("a landed submission");
            match sub.beside {
                // Record 0100 D3: by the landing of what carried its work.
                _ if sub.landed_elsewhere && by.is_some_and(CarriedBy::landed) => {
                    let by = by.expect("what carried its work").name();
                    writeln!(out, "{}", words::landed_carried(branch, &to, &by))?
                }
                // 122: no verdict of its own landed it.
                _ if sub.landed_elsewhere => {
                    writeln!(out, "{}", words::landed_elsewhere(branch, &to))?
                }
                // 140: nothing was run, so nothing passed.
                Some(Beside::Landed) => writeln!(out, "{}", words::landed_beside(&to))?,
                _ => writeln!(out, "passed · landed as {}", to.short())?,
            }
        }
        State::Failed => {
            let t = sub.tested.clone().expect("a blamed submission was tested");
            let Failure {
                ran,
                failed_by,
                timed_out,
                cut,
                not_run,
                flaky,
                could_not,
            } = Failure::of(sub);
            if timed_out && cut.is_empty() {
                writeln!(out, "timed out")?;
            } else if timed_out {
                writeln!(out, "{}", words::cut_off(&cut))?;
            }
            if !ran.is_empty() {
                let named: Vec<String> = ran.iter().map(|f| failed_name(f)).collect();
                writeln!(out, "failed: {}", named.join(", "))?;
            }
            // C.6, 20 (design D5 of the change
            // `a-signed-run-without-a-sandbox-proves-its-passes`): a check
            // whose key the combination's tree does not bear out, and why.
            for f in &ran {
                if let Some(why) = &f.unsigned {
                    writeln!(out, "  {}", words::not_borne_out(&f.task, why))?;
                }
            }
            for f in &failed_by {
                writeln!(out, "  {}", words::failed_by(&failed_name(f), &f.failed_by))?;
            }
            if !not_run.is_empty() {
                writeln!(out, "not run: {}", not_run.join(", "))?;
            }
            // ADR 0044: a required check its task runner keeps out of CI.
            let combination_log: Vec<_> = log
                .iter()
                .filter(|l| matches!(&l.event, Event::Detail { combination, .. } if *combination == t.combination))
                .cloned()
                .collect();
            for task in sluice_core::ops::kept_out(&combination_log) {
                if not_run.contains(&task.as_str()) {
                    writeln!(
                        out,
                        "  {}",
                        sluice_core::ops::kept_out_warning(branch, &task)
                    )?;
                }
            }
            // 139, design D8: a run of it whose executor could not read
            // what its sandbox refused it, read as refused nothing.
            let mut unread: Vec<&str> = Vec::new();
            for l in &combination_log {
                if let Event::Detail {
                    refusals_unread: Some(why),
                    ..
                } = &l.event
                {
                    if !unread.contains(&why.as_str()) {
                        unread.push(why);
                    }
                }
            }
            for why in unread {
                writeln!(out, "  {}", words::refusals_unread(why))?;
            }
            // Could-not-run-1 115: a test that could not run, and why.
            for f in &could_not {
                let why = f.not_run.as_deref().unwrap_or_default();
                writeln!(out, "  {}", words::could_not_run(&f.task, why))?;
            }
            writeln!(
                out,
                "  on {} ({})",
                combination(&t.submissions),
                t.combination.short()
            )?;
            let ahead: Vec<u64> = t.submissions.iter().copied().filter(|i| *i != id).collect();
            match ahead.len() {
                0 => {}
                1 => writeln!(
                    out,
                    "  {} passed on its own, so the failure is from this submission",
                    name(ahead[0])
                )?,
                _ => writeln!(
                    out,
                    "  {} passed ahead of it, so the failure is from this submission",
                    ahead
                        .iter()
                        .map(|i| name(*i))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?,
            }
            // 20: where what each failed task printed is kept; for one read
            // as flaky, the run it contradicts and where that is kept (19).
            for f in &sub.failed {
                let Some(k) = &f.flaky else {
                    if let Some(at) = &f.log {
                        writeln!(out, "  log of {}: {at}", failed_name(f))?;
                    }
                    continue;
                };
                let word = words::outcome;
                writeln!(out, "  {}", words::flaky(&f.task, k))?;
                if let Some(at) = &f.log {
                    writeln!(out, "  log of this run: {at}")?;
                }
                if let Some(at) = &k.other.log {
                    writeln!(out, "  log of the run that {}: {at}", word(k.other.outcome))?;
                }
                // Could-not-run-1 116: a fail the host caused is the
                // operator's to set aside.
                if k.other.outcome == Outcome::Fail {
                    writeln!(out, "  {}", words::set_aside_hint(&f.task, &k.other.at))?;
                }
            }
            if !flaky.is_empty() {
                writeln!(
                    out,
                    "  {branch} reads a test that passed and failed on the same inputs as failed \
                     (flaky = \"fail\")"
                )?;
            }
            for w in ways_on(s, sub, branch)? {
                writeln!(out, "{w}")?;
            }
        }
        State::Conflict => {
            let c = sub.conflict.clone().expect("a conflicting submission");
            writeln!(out, "conflict: {}", c.paths.join(", "))?;
            for w in &c.with {
                writeln!(
                    out,
                    "  conflicts with {} ({}, {})",
                    name(w.id),
                    w.from.agent,
                    w.from.worktree
                )?;
            }
            for w in ways_on(s, sub, branch)? {
                writeln!(out, "{w}")?;
            }
        }
        // Record 0100 D3: what carried its work stopped short, said as
        // what carried it, with where to read it.
        State::Withdrawn if by.is_some() => {
            let by = by.expect("what carried its work");
            writeln!(
                out,
                "{}",
                words::carrier_stopped(id, &left(sub), &by.name(), &by.how())
            )?;
            if let CarriedBy::Entry(e) = by {
                writeln!(out, "sluice status {}", e.id)?;
            }
        }
        _ => writeln!(out, "{}", sub.state.word())?,
    }
    Ok(exit_for(sub))
}

/// Record 0100 D1, D3: what a wait followed in its own submission's stead,
/// a later submission of this host's or the pull request, and how it
/// stands, which the wait's document names as `carried_by`.
#[derive(Clone, Debug)]
enum CarriedBy {
    /// A later submission that carries its work (67).
    Entry(Submission),
    /// Its pull request, with no submission of it on this host that the
    /// wait follows (137).
    Request(u64, RequestEnd),
}

/// How a pull request a wait follows stands, in its `carried_by`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestEnd {
    Open,
    Landed,
    Failed,
    Conflict,
    Closed,
}

impl CarriedBy {
    /// Its `carried_by`: `{"id","state"}` for a submission, and
    /// `{"pull_request","state"}` for a request.
    fn json(&self) -> serde_json::Value {
        match self {
            CarriedBy::Entry(e) => serde_json::json!({ "id": e.id, "state": e.state }),
            CarriedBy::Request(n, end) => {
                let state = match end {
                    RequestEnd::Open => "open",
                    RequestEnd::Landed => "landed",
                    RequestEnd::Failed => "failed",
                    RequestEnd::Conflict => "conflict",
                    RequestEnd::Closed => "closed",
                };
                serde_json::json!({ "pull_request": n, "state": state })
            }
        }
    }

    /// What it is called: `#<b>`, or `pull request #<n>`.
    fn name(&self) -> String {
        match self {
            CarriedBy::Entry(e) => format!("#{}", e.id),
            CarriedBy::Request(n, _) => format!("pull request #{n}"),
        }
    }

    /// Whether it landed.
    fn landed(&self) -> bool {
        match self {
            CarriedBy::Entry(e) => e.state == State::Landed,
            CarriedBy::Request(_, end) => *end == RequestEnd::Landed,
        }
    }

    /// How it stopped short, in words.
    fn how(&self) -> String {
        match self {
            CarriedBy::Entry(e) => match e.state {
                State::Failed => "failed".into(),
                State::Conflict => "is in conflict".into(),
                State::Withdrawn => format!("was {}", left(e)),
                state => format!("is {}", state.word()),
            },
            CarriedBy::Request(_, RequestEnd::Closed) => "was closed without landing".into(),
            CarriedBy::Request(_, RequestEnd::Failed | RequestEnd::Conflict) => {
                "was sent back".into()
            }
            CarriedBy::Request(_, RequestEnd::Landed) => "landed".into(),
            CarriedBy::Request(_, RequestEnd::Open) => "is open".into(),
        }
    }
}

/// `doc`, a wait's document, with `carried_by` where it followed what
/// carried its work (record 0100 D3).
fn with_carried(mut doc: serde_json::Value, by: Option<&CarriedBy>) -> serde_json::Value {
    if let Some(by) = by {
        doc["carried_by"] = by.json();
    }
    doc
}

/// How `sub` left the queue, in a wait's words: `withdrawn by <who>`, or,
/// for a hand-over's withdrawal, `handed over to <host>` (137).
fn left(sub: &Submission) -> String {
    match sub.withdrawn.as_ref() {
        Some(w) => match &w.handed_over_to {
            Some(host) => format!("handed over to {host}"),
            None => format!("withdrawn by {}", w.by),
        },
        None => "withdrawn by its submitter".to_string(),
    }
}

/// Record 0100 D2: the wait's own submission `own`'s end, once `by`, what
/// carried its work, is decided: landed, as the commit that landed its
/// head, where its branch, read as a round reads it, holds that head,
/// whichever landing took it; otherwise as its record stands, withdrawn,
/// even where what carried its work landed, never a landing the branch
/// does not show. The commit that landed it is the landing of `by` where
/// `by` is a submission that landed, as record 0100 has it, "by the
/// carrier's landing", which a branch landing by fast-forward would
/// otherwise give as `own`'s head itself; else the oldest of the branch's
/// first-parent history holding its head (record 0085). Its record kept
/// is never changed (5, 67, 122).
fn own_end(s: &Sluice<'_>, own: Submission, by: Option<&CarriedBy>) -> Result<Submission> {
    if own.state == State::Landed {
        return Ok(own);
    }
    let branch = s.seams().branch;
    let to = match branch.read(&own.branch)? {
        Some(tip) => branch.landed_by(&tip, &own.head)?,
        None => None,
    };
    let carrier = match by {
        Some(CarriedBy::Entry(e)) if e.state == State::Landed => e.landed_as.clone(),
        _ => None,
    };
    Ok(match to.map(|to| carrier.unwrap_or(to)) {
        Some(to) => Submission {
            state: State::Landed,
            landed_as: Some(to),
            landed_elsewhere: true,
            ..own
        },
        None => own,
    })
}

/// What a blamed submission's record says failed, as its report and its
/// ways on read it.
struct Failure<'a> {
    /// The checks that ran and failed, each with its key.
    ran: Vec<&'a FailedTask>,
    /// 149: the checks failed by the tasks they depend on, with no key of
    /// their own and never run: said after the tasks that failed them,
    /// never as not run or cut off.
    failed_by: Vec<&'a FailedTask>,
    /// Its run's time limit ended it (time-limit-1 66).
    timed_out: bool,
    /// The checks the time limit cut off: with no key, and able to run.
    cut: Vec<&'a str>,
    /// The checks left without a result (15), not cut off: not run, most
    /// often behind a check that failed.
    not_run: Vec<&'a str>,
    /// 19: read as failed for its key holding a pass and a fail.
    flaky: Vec<&'a str>,
    /// Could-not-run-1 115: the checks the host could not run.
    could_not: Vec<&'a FailedTask>,
}

impl<'a> Failure<'a> {
    fn of(sub: &'a Submission) -> Failure<'a> {
        let ran: Vec<&FailedTask> = sub.failed.iter().filter(|f| !f.hash.is_empty()).collect();
        let failed_by: Vec<&FailedTask> = sub
            .failed
            .iter()
            .filter(|f| !f.failed_by.is_empty())
            .collect();
        let timed_out = sub.result.as_deref() == Some("timed out");
        let keyless = |f: &&FailedTask| f.hash.is_empty() && f.failed_by.is_empty();
        let cut: Vec<&str> = sub
            .failed
            .iter()
            .filter(|f| timed_out && keyless(f) && f.not_run.is_none())
            .map(|f| f.task.as_str())
            .collect();
        let not_run: Vec<&str> = sub
            .failed
            .iter()
            .filter(keyless)
            .map(|f| f.task.as_str())
            .filter(|t| !cut.contains(t))
            .collect();
        let flaky: Vec<&str> = ran
            .iter()
            .filter(|f| f.flaky.is_some())
            .map(|f| f.task.as_str())
            .collect();
        let could_not: Vec<&FailedTask> =
            sub.failed.iter().filter(|f| f.not_run.is_some()).collect();
        Failure {
            ran,
            failed_by,
            timed_out,
            cut,
            not_run,
            flaky,
            could_not,
        }
    }
}

/// The ways on of a failed or conflicting submission to `branch`, in the
/// order its report gives them, for whoever moves it on: the agent working
/// in the worktree it came from, or, for a branch submitted by name, its
/// caller, who lands a fix on the branch and submits it by name again
/// (D10). A fix by name to a conflict merges `branch` at the revision the
/// submission was tested on, or at the branch's own where it was not.
/// None for any other state.
fn ways_on(s: &Sluice<'_>, sub: &Submission, branch: &str) -> Result<Vec<String>> {
    let mover = words::Mover::of(&sub.from, branch);
    let mut ways = Vec::new();
    match sub.state {
        State::Failed => {
            let f = Failure::of(sub);
            if !f.could_not.is_empty() {
                let tasks: Vec<&str> = f.could_not.iter().map(|f| f.task.as_str()).collect();
                ways.push(words::could_not_run_way_on(mover, &tasks));
            }
            if f.timed_out {
                ways.push(words::timed_out_way_on(mover, &f.cut));
            }
            if f.not_run.len() > f.could_not.len() || f.ran.len() > f.flaky.len() {
                ways.push(words::fix_way_on(mover));
            }
            if !f.flaky.is_empty() {
                let other_failed = f
                    .ran
                    .iter()
                    .filter_map(|f| f.flaky.as_ref())
                    .all(|k| k.other.outcome == Outcome::Fail);
                ways.push(words::flaky_way_on(&f.flaky, other_failed));
            }
        }
        State::Conflict => {
            let at = match (&mover, &sub.tested) {
                (words::Mover::Agent(_), _) => None,
                (_, Some(t)) => Some(t.on.clone()),
                (_, None) => s.seams().branch.read(branch)?,
            };
            ways.push(words::conflict_way_on(mover, branch, at.as_ref()));
        }
        _ => {}
    }
    Ok(ways)
}

/// 51, 135: `sluice inbox`: hand over every outcome still owed to the
/// readers of `worktree`, or with `new` only those no way has handed over
/// yet: print each in the notice the round pushes and what `sluice wait
/// <id>` prints, then, unless `confirms`, the line naming the key to
/// confirm it by, and record each handed over by `inbox` to it, confirmed
/// with it where `confirms`. What it prints is written before it records,
/// so a collect stopped between them hands it over again; each branch it
/// collects from is locked meanwhile, so no round pushes the same outcome
/// (record 0102; design D8).
fn inbox(
    s: &Sluice<'_>,
    worktree: &str,
    json: bool,
    confirms: bool,
    new: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let Some((me, to)) = recipient(s, worktree)? else {
        if json {
            writeln!(out, "{}", serde_json::json!({ "outcomes": [] }))?;
        }
        return Ok(0);
    };
    let collect = if new { Collect::New } else { Collect::All };
    let owed = s.owed(&me, collect)?;
    for u in &owed.unread {
        telemetry::error(&format!(
            "sluice: could not read {}'s records: {}",
            u.branch, u.error
        ));
    }
    if json {
        // Each element is the record with the notice the round pushes for
        // it, and its delivery as this run leaves it (D8, D13).
        let outcomes = owed
            .outcomes
            .iter()
            .map(|o| {
                let mut doc = decided_document(&o.submission, Some(&o.decided))?;
                doc["delivery"] = o.left("inbox", &owed.at, confirms).to_json();
                Ok(doc)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut doc = serde_json::json!({ "outcomes": outcomes });
        if !owed.unread.is_empty() {
            let unread: Vec<_> = owed
                .unread
                .iter()
                .map(|u| serde_json::json!({ "branch": u.branch, "error": u.error }))
                .collect();
            doc["unread"] = serde_json::Value::Array(unread);
        }
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        let names = BTreeMap::new();
        for (i, o) in owed.outcomes.iter().enumerate() {
            let sub = &o.submission;
            if i > 0 {
                writeln!(out)?;
            }
            writeln!(
                out,
                "{}",
                sluice_core::notice(sub.id, &o.branch, &o.decided)
            )?;
            report(s, sub, &o.branch, &names, None, None, None, false, out)?;
            if !confirms {
                writeln!(out, "{}", words::confirm_line(o))?;
            }
        }
    }
    out.flush()?;
    let unread = !owed.unread.is_empty();
    owed.hand("inbox", &to, confirms)?;
    Ok(if unread { INTERNAL } else { 0 })
}

/// 51: `sluice confirm <key>...`: confirm each key for the readers
/// `worktree` reads as, or, `as_operator`, for each outcome's own reader,
/// and say each: confirmed, confirmed before and when, not found, or owed
/// to another reader, in the order given. The exit is the worst of the
/// keys', 3 over 2 over 1: a branch it could not read where a key was not
/// found is 3 (rule core.11). Under `--json`, one document of the keys
/// confirmed, or the error document of the worst refusal (design D7).
fn confirm(
    s: &Sluice<'_>,
    keys: &[String],
    as_operator: bool,
    worktree: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let who = if as_operator {
        sluice_core::ConfirmAs::Operator
    } else {
        match recipient(s, worktree)? {
            Some((me, _)) => sluice_core::ConfirmAs::Folder(me),
            None => {
                let text = "this folder is on no branch, so it is owed nothing to confirm";
                let next = [
                    "sluice inbox".to_string(),
                    "sluice confirm <key> --as operator".to_string(),
                ];
                told::tell(out, json, Kind::Refused, text, &next)?;
                return Ok(CONFLICT);
            }
        }
    };
    let got = s.confirm(keys, &who)?;
    let unread = !got.unread.is_empty();
    for u in &got.unread {
        telemetry::error(&format!(
            "sluice: could not read {}'s records: {}",
            u.branch, u.error
        ));
    }
    let mut code = 0;
    let mut worst: Option<(Kind, String, Vec<String>)> = None;
    let mut confirmed = Vec::new();
    for answer in &got.answers {
        use sluice_core::Confirmation as C;
        match answer {
            C::Confirmed {
                key,
                branch,
                id,
                reader,
                at,
            }
            | C::Before {
                key,
                branch,
                id,
                reader,
                at,
            } => {
                let first = matches!(answer, C::Confirmed { .. });
                confirmed.push(serde_json::json!({
                    "key": key, "branch": branch, "reader": reader, "at": at, "first": first,
                }));
                if !json {
                    match first {
                        true => writeln!(out, "{}", words::confirmed(key, *id, branch))?,
                        false => writeln!(out, "{key} was confirmed at {at}")?,
                    }
                }
            }
            C::NotFound { key } if unread => {
                code = code.max(INTERNAL);
                let text = format!(
                    "no branch read holds an outcome under {key}, and a branch could not be read"
                );
                worst = Some((Kind::Internal, text, vec![format!("sluice confirm {key}")]));
            }
            C::NotFound { key } => {
                let text = format!("no outcome is owed under {key}: sluice inbox gives the keys");
                if code < FAILED {
                    worst = Some((Kind::NotFound, text.clone(), vec!["sluice inbox".into()]));
                }
                code = code.max(FAILED);
                if !json {
                    writeln!(out, "{text}")?;
                }
            }
            C::Another {
                key,
                branch,
                readers,
            } => {
                let text = format!(
                    "refused: {key} on {branch} is owed to {}, not this worktree: sluice \
                     confirm {key} --as operator confirms it for them",
                    readers.join(", ")
                );
                if code < CONFLICT {
                    worst = Some((
                        Kind::Refused,
                        text.clone(),
                        vec![format!("sluice confirm {key} --as operator")],
                    ));
                }
                code = code.max(CONFLICT);
                if !json {
                    writeln!(out, "{text}")?;
                }
            }
        }
    }
    if json {
        match &worst {
            Some((kind, text, next)) => told::tell(out, true, *kind, text, next)?,
            None => writeln!(
                out,
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "confirmed": confirmed }))?
            )?,
        }
    } else if let Some((Kind::Internal, text, _)) = &worst {
        writeln!(out, "{text}")?;
    }
    Ok(code)
}

/// 51: who works in `worktree`, and the name an outcome is delivered to
/// there: the session as its driver names it, else the worktree's path,
/// else, where no files are checked out, the agent (record 0099's D3).
/// None where its `HEAD` names no branch, a place that submitted nothing
/// (3). A folder with no files checked out is the worktree on the branch
/// its `HEAD` names, read with neither its files nor its `HEAD`'s commit
/// (2; rule core.48).
fn recipient(s: &Sluice<'_>, worktree: &str) -> Result<Option<(Submitter, String)>> {
    let Some(me) = s.seams().driver.recipient(worktree)? else {
        return Ok(None);
    };
    let to = me
        .session
        .clone()
        .or_else(|| me.path.clone())
        .unwrap_or_else(|| me.agent.clone());
    Ok(Some((me, to)))
}

/// 51: `sub`, decided, locked and ready to mark delivered with the name to
/// mark, when it is `worktree`'s own and pending; None otherwise, saying
/// nothing, as for another worktree's, wherever it is read, and when what
/// it needs could not be read, which is said and leaves it pending for the
/// next carrier (record 0099; rule core.48).
fn delivering<'s, 'a>(
    s: &'s Sluice<'a>,
    worktree: &str,
    sub: &Submission,
) -> Option<(Owed<'s, 'a>, String)> {
    let one = || Collect::One {
        branch: sub.branch.clone(),
        id: sub.id,
    };
    let found = recipient(s, worktree)
        .and_then(|r| r.map(|(me, to)| Ok((s.owed(&me, one())?, to))).transpose());
    match found {
        Ok(Some((pending, to))) if !pending.outcomes.is_empty() => Some((pending, to)),
        Ok(_) => None,
        Err(e) => {
            telemetry::error(&format!(
                "sluice: could not read whether #{} is owed here, so sluice inbox hands it over: {e:#}",
                sub.id
            ));
            None
        }
    }
}

/// Whether `branch`, at its revision, takes its work as pull requests
/// (C.7 `pull_requests`).
fn takes_pulls(s: &Sluice<'_>, branch: &str) -> Result<bool> {
    let Some(tip) = s.seams().branch.read(branch)? else {
        return Ok(false);
    };
    Ok(s.rules_at(branch, &tip)?
        .is_some_and(|r| r.pull_requests.is_some()))
}

/// `sluice submit --status`: where `worktree`'s last submission to
/// `branch` stands.
/// A refusal's words for a number that names no submission, on the
/// branch the command was given, or on any, and the command that moves
/// its reader on (26; record 0114).
fn no_submission(id: u64, branch: Option<&str>) -> (String, Vec<String>) {
    match branch {
        Some(b) => (
            format!("no submission {id} on {b}: sluice status {b} lists its queue"),
            vec![format!("sluice status {b}")],
        ),
        None => (
            format!("no submission {id}: sluice overview lists the queues"),
            vec!["sluice overview".into()],
        ),
    }
}

/// What `sluice wait <n>` and `sluice status <n>` say of a number that
/// names no submission, on the branch they were given, or on any, every
/// branch read, and the command that moves their reader on (26; record
/// 0114).
fn no_such(id: u64, branch: Option<&str>) -> (String, Vec<String>) {
    match branch {
        Some(b) => (
            format!("no submission {id} on {b}"),
            vec![format!("sluice status {b}")],
        ),
        None => (
            format!("no submission {id}"),
            vec!["sluice overview".into()],
        ),
    }
}

/// Submission `id` as last written, read on `branch` alone where the
/// command was given it, and on every branch where it was not (26; record
/// 0114).
fn state(s: &Sluice<'_>, id: u64, branch: Option<&str>) -> Result<Option<Submission>> {
    match branch {
        Some(b) => s.state_on(b, id),
        None => s.state(id),
    }
}

/// `ask` for submission `id`, naming `branch` where the command was given
/// it, so every command a refusal or a line suggests for the submission
/// reads the one it was asked of (26; record 0114).
fn asked(ask: &str, id: u64, branch: Option<&str>) -> String {
    match branch {
        Some(b) => format!("{ask} {id} --branch {b}"),
        None => format!("{ask} {id}"),
    }
}

/// Submission `id`, looked up on `branch` alone where the command was
/// given it, and on every branch where it was not (26; record 0114).
fn lookup(s: &Sluice<'_>, id: u64, branch: Option<&str>) -> Result<Lookup> {
    match branch {
        Some(b) => s.lookup_on(b, id),
        None => s.lookup(id),
    }
}

/// Why `--branch` beside `target`, a branch, is a command line sluice
/// cannot read (record 0114).
fn branch_goes_with_a_number(target: &str) -> String {
    format!("--branch names the branch of a submission's number; {target} is a branch")
}

/// 26: the words for submission `id` on none of the branches sluice
/// could read, while those `unread` could not be: that line, then each
/// branch with the words it was refused with, whole (133), as the
/// overview words a branch it could not read.
fn unfound_lines(id: u64, unread: &[Unread]) -> Vec<String> {
    std::iter::once(format!(
        "submission {id} is on none of the branches sluice could read"
    ))
    .chain(
        unread
            .iter()
            .map(|u| format!("{} could not be read: {}", u.branch, u.error)),
    )
    .collect()
}

/// 26: say that submission `id` is on none of the branches sluice could
/// read, naming each it could not, and never that there is no such
/// submission, since whether there is one is not known (rule core.11);
/// exit [`INTERNAL`]. Under `--json`, the internal error document, its
/// message the lines joined by `; `, next `sluice overview`.
fn unfound(id: u64, unread: &[Unread], json: bool, out: &mut dyn Write) -> Result<i32> {
    let lines = unfound_lines(id, unread);
    let text = lines.join(if json { "; " } else { "\n" });
    let next = ["sluice overview".to_string()];
    told::tell(out, json, Kind::Internal, &text, &next)?;
    Ok(INTERNAL)
}

/// 26, record 0114: the words for submission `id` on more than one
/// branch, which names none of them alone: the refusal naming each branch
/// that holds it, in the records' order, and the command `ask` with
/// `--branch` for the first, then each branch that could not be read with
/// the words it was refused with, as [`unfound_lines`] words them.
fn several_lines(id: u64, ask: &str, several: &Several) -> Vec<String> {
    let first = several.branches.first().map_or("", String::as_str);
    std::iter::once(format!(
        "refused: submission {id} is on more than one branch: {}; name one with --branch, \
         as {ask} {id} --branch {first}",
        several.branches.join(", ")
    ))
    .chain(
        several
            .unread
            .iter()
            .map(|u| format!("{} could not be read: {}", u.branch, u.error)),
    )
    .collect()
}

/// 26, record 0114: refuse submission `id`, which more than one branch
/// holds, naming each and each branch that could not be read; exit
/// `code`, `2` as every refusal of `status`, `amend` and `withdraw`, and
/// [`USAGE`] for a wait, whose `2` is a conflict. Under `--json`, the
/// refused document, its message the lines joined by `; `, and next
/// `ask` with `--branch` once for each branch that holds it.
fn refuse_several(
    id: u64,
    ask: &str,
    several: &Several,
    json: bool,
    code: i32,
    out: &mut dyn Write,
) -> Result<i32> {
    let lines = several_lines(id, ask, several);
    let text = lines.join(if json { "; " } else { "\n" });
    let next: Vec<String> = several
        .branches
        .iter()
        .map(|b| format!("{ask} {id} --branch {b}"))
        .collect();
    told::tell(out, json, Kind::Refused, &text, &next)?;
    Ok(code)
}

/// The commands a refusal for another's submission `id` moves on with:
/// `ask` again as its agent, or as the operator. Read on `branch` alone
/// where the command was given it (record 0114).
fn owner_next(s: &Sluice<'_>, id: u64, branch: Option<&str>, ask: &str) -> Result<Vec<String>> {
    let agent = match state(s, id, branch)? {
        Some(sub) => sub.from.agent,
        None => "<agent>".into(),
    };
    Ok(vec![
        format!("{ask} --as {agent}"),
        format!("{ask} --as operator"),
    ])
}

/// Who may ask for submission `id`'s change, for a refusal to say, read
/// on `branch` alone where the command was given it (record 0114).
fn owner(s: &Sluice<'_>, id: u64, branch: Option<&str>) -> Result<String> {
    Ok(match state(s, id, branch)? {
        Some(sub) => format!("ask --as {}, its agent, or --as operator", sub.from.agent),
        None => "ask --as its agent, or --as operator".into(),
    })
}

/// This worktree's last submission to `branch`, as `submit --status` and
/// `sluice wait <branch>` find it: from the live records first, the
/// branch's history only when they hold nothing from it. `Err` with the
/// worktree's branch when it submitted nothing there.
fn last_submitted(
    s: &Sluice<'_>,
    worktree: &str,
    branch: &str,
) -> Result<std::result::Result<Submission, String>> {
    let from = s.seams().driver.session(worktree)?.from;
    let latest = |subs: &[Submission]| {
        subs.iter()
            .rev()
            .find(|x| x.from.worktree == from.worktree)
            .cloned()
    };
    let found = match latest(&s.status(branch)?.submissions) {
        Some(sub) => Some(sub),
        None => latest(&s.history(branch)?),
    };
    Ok(found.ok_or(from.worktree))
}

/// `doc` with `sends`: the revision a submit from `worktree` would send
/// now, as the core decides it (124), so a driver reading `--status
/// --json` can tell whether the submission stands at it without deciding
/// that itself (record 0065; rule core.27).
fn with_sends(
    mut doc: serde_json::Value,
    s: &Sluice<'_>,
    worktree: &str,
) -> Result<serde_json::Value> {
    let session = s.seams().driver.session(worktree)?;
    doc["sends"] = serde_json::to_value(s.submitted_head(&session)?.head)?;
    Ok(doc)
}

fn submitted(
    s: &Sluice<'_>,
    clock: &dyn Clock,
    worktree: &str,
    branch: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let sub = match last_submitted(s, worktree, branch)? {
        Ok(sub) => sub,
        Err(from) => {
            let text = format!("nothing submitted from {from} to {branch}");
            let next = [format!("sluice submit {branch}")];
            told::tell(out, json, Kind::NotFound, &text, &next)?;
            return Ok(1);
        }
    };
    let status = s.status(branch)?;
    if json {
        let doc = with_sends(serde_json::to_value(&sub)?, s, worktree)?;
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        let names = status_names(&status, std::slice::from_ref(&sub));
        let testing = progress::lines(s, branch, Some(sub.id), clock.now(), Some(&names))?;
        write_status(&status, None, &testing, &[sub], &names, None, out)?;
    }
    Ok(0)
}

/// What `submit_pull` does with a worktree's pull request.
#[derive(Clone, Copy)]
enum PullMode<'a> {
    /// Open or update it, titled by `subject` where given, then follow it
    /// to its outcome unless `no_wait`.
    Open {
        no_wait: bool,
        subject: Option<&'a str>,
    },
    /// Only say where it stands.
    Status,
    /// Follow it to its outcome, or until `deadline`: `sluice wait
    /// <branch>` (ADR 0041).
    Wait {
        deadline: Option<chrono::DateTime<chrono::Utc>>,
    },
    /// Follow pull request `number` as [`PullMode::Wait`] follows the one
    /// open, for `own`: a wait whose submission was withdrawn while the
    /// request carries its work on (67, 137). It ends with `own`, never
    /// with a submission of the request, as [`own_end`] has it (record
    /// 0100 D2, D4).
    Follow {
        number: u64,
        own: &'a Submission,
        deadline: Option<chrono::DateTime<chrono::Utc>>,
    },
}

impl PullMode<'_> {
    /// Only follows what stands: `sluice wait`, whatever it follows.
    fn waits(&self) -> bool {
        matches!(self, PullMode::Wait { .. } | PullMode::Follow { .. })
    }

    fn deadline(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            PullMode::Wait { deadline } | PullMode::Follow { deadline, .. } => *deadline,
            _ => None,
        }
    }
}

/// `sluice submit` to a branch with pull requests (C.5 pull request):
/// push the worktree's branch and open or update its pull request, which
/// waits for its conditions; then follow it, reporting each state it
/// reaches, to its outcome. [`PullMode::Status`] only says where it
/// stands; [`PullMode::Wait`] follows the one open. While it waits, each
/// read is tried again under the retry policy for the wait's five minutes
/// (134), and one that still fails as the forge could not answer ends the
/// wait as it could not read. Where `sluice run` lives on the host, it
/// waits as [`wait_on`] does, running no round, and takes requests in only
/// at a submit's first pass, which is its own act (135). An outcome it
/// reports that is still pending is marked delivered to the worktree, whose
/// branch the request was opened from, as [`await_submission`] marks one (51).
/// A branch named (124) opens the request from that branch, reading no
/// worktree, and its outcome is marked delivered to none: the request
/// carries it (C.5; record 0090). A waiting submit and `sluice wait
/// <branch>` take as their own the first submission of the request they
/// read; once the request is taken in as another, they wait on their own
/// as `sluice wait <id>` does, `tick` its reader's (record 0100 D4).
#[allow(clippy::too_many_arguments)]
fn submit_pull(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    head: &SubmitHead,
    branch: &str,
    mode: PullMode<'_>,
    json: bool,
    confirms: Confirms,
    pause: &dyn Pause,
    tick: Duration,
    out: &mut dyn Write,
) -> Result<i32> {
    let seams = s.seams();
    // The place the request is found by, and the outcome delivered to: a
    // worktree's; none for a branch named, whose way is the request's.
    let worktree = head.name();
    let delivered_to = match head {
        SubmitHead::Worktree(w) => Some(w.as_str()),
        SubmitHead::Branch(_) => None,
    };
    let status = matches!(mode, PullMode::Status);
    let no_wait = matches!(mode, PullMode::Open { no_wait: true, .. });
    let opens = matches!(mode, PullMode::Open { .. });
    // Record 0100 D4: the submission a wait follows the request for.
    let follows_for = match mode {
        PullMode::Follow { own, .. } => Some(own),
        _ => None,
    };
    let again = match follows_for {
        Some(own) => format!("sluice wait {}", own.id),
        None => format!("sluice wait {branch}"),
    };
    // Record 0100 D4: a waiting submit's and `sluice wait <branch>`'s own
    // submission, the first of the request it reads.
    let mut own: Option<Submission> = None;
    let takes_own = !status && !no_wait && follows_for.is_none();
    // Waits on `own` as `sluice wait <id>` does, once the request is taken
    // in as another submission.
    let await_own = |own: &Submission, copy: Option<&CopyDiffers>, out: &mut dyn Write| {
        let own = s
            .state_on(&own.branch, own.id)?
            .unwrap_or_else(|| own.clone());
        let waiting = Waiting {
            host,
            clock,
            pulls: Some(pulls),
            follows: pulls,
            deadline: mode.deadline(),
            json,
            pause,
            tick,
            copy,
            worktree: delivered_to,
            confirms,
            named: None,
        };
        await_submission(s, &waiting, own, None, out)
    };
    // 124: how the host's copy differed from what the submit opened.
    let mut copy = None;
    let number = if let PullMode::Follow { number, .. } = mode {
        number
    } else if !opens {
        let found = match pulls.find(seams, clock, branch, worktree) {
            Ok(found) => found,
            Err(e) => {
                let PullMode::Wait { .. } = mode else {
                    return Err(e);
                };
                if waited_out(&e) && !INTERRUPTED.load(Ordering::SeqCst) {
                    return Ok(WAITING);
                }
                let what = format!("the pull request from {worktree} into {branch}");
                let code = could_not_read(&unread(e)?, &what, &again, json, out)?;
                // Ctrl-C ended the retry policy's pauses.
                if INTERRUPTED.load(Ordering::SeqCst) {
                    return Ok(STOPPED);
                }
                return Ok(code);
            }
        };
        match found {
            Some(n) => n,
            None => {
                let text = format!("no pull request from {worktree} into {branch}");
                let next = [format!("sluice submit {branch}")];
                told::tell(out, json, Kind::NotFound, &text, &next)?;
                return Ok(1);
            }
        }
    } else {
        let subject = match mode {
            PullMode::Open { subject, .. } => subject,
            _ => None,
        };
        match pulls.open(seams, clock, head, branch, subject)? {
            Ok(o) => {
                if !json {
                    writeln!(out, "{}", o.said)?;
                }
                copy = o.copy;
                o.number
            }
            Err(r) => {
                let named = head_name(s, head);
                let text = refusal(&r, worktree, &named, branch, &())?;
                told::tell(out, json, Kind::Refused, &text, &refusal_next(&r, branch))?;
                return Ok(CONFLICT);
            }
        }
    };
    let mut last: Option<(String, Vec<String>)> = None;
    // 137: where the pull request stands on the host that takes it in, as
    // last read and as last said, where that is another host.
    let mut there: Option<There> = None;
    let mut said_there = None;
    // 121: the branch's hold as last said, so each is said once.
    let mut said_hold = None;
    let what = format!("#{number}");
    // The submission the pull request was last read taken in as.
    let mut taken: Option<u64> = None;
    let probe = driven_by(host);
    let mut first = true;
    // 137: a wait lets go of the intake mark it holds when it ends.
    let _let_go = LetGo(Some(pulls));
    loop {
        // 135: probed once a pass. The first pass's intake is a submit's
        // own, and `--status`'s: the answer names what it took in. Every
        // other, and all of `sluice wait <branch>`'s, is the driver's
        // while it lives.
        let driven = probe();
        if !driven || first && !mode.waits() {
            // 137: a wait on a host where no driver lives shows itself by
            // the intake mark; a submit that does not wait, `--status`, and
            // a submit's first pass beside a live driver push none.
            let taker = match driven || no_wait || status {
                true => Taker::Step,
                false => Taker::Wait,
            };
            // The user, 2026-10-28: a submit that does not wait fails at
            // once on a spent allowance, naming when it ends (134).
            match intake(pulls, seams, clock, branch, taker, json, out) {
                Some(e) if no_wait => return Err(e),
                Some(_) if INTERRUPTED.load(Ordering::SeqCst) => return Ok(STOPPED),
                _ => {}
            }
        }
        first = false;
        let followed = match pulls.follow(seams, clock, branch, number) {
            Ok(f) => Some(f),
            Err(e) if no_wait || status => return Err(e),
            // Ctrl-C ended the retry policy's pauses: stopped, below.
            Err(_) if INTERRUPTED.load(Ordering::SeqCst) => None,
            Err(e) if waited_out(&e) => return Ok(WAITING),
            Err(e) => return could_not_read(&unread(e)?, &what, &again, json, out),
        };
        match followed {
            Some(Followed::Done(sub, names)) => {
                if status && json {
                    let doc = with_sends(serde_json::to_value(&sub)?, s, worktree)?;
                    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                    return Ok(0);
                }
                if status {
                    let status = s.status(branch)?;
                    let names = status_names(&status, std::slice::from_ref(&sub));
                    let testing =
                        progress::lines(s, branch, Some(sub.id), clock.now(), Some(&names))?;
                    write_status(
                        &status,
                        None,
                        &testing,
                        std::slice::from_ref(&sub),
                        &names,
                        None,
                        out,
                    )?;
                    return Ok(0);
                }
                // Record 0100 D2, D4: a wait that follows the request for its
                // own ends with its own, the submission decided what
                // carried its work, or the request where it is its own.
                if let Some(own) = follows_for {
                    let (end, by) = match sub.id == own.id {
                        true => {
                            let end = match sub.state {
                                State::Landed => RequestEnd::Landed,
                                State::Conflict => RequestEnd::Conflict,
                                _ => RequestEnd::Failed,
                            };
                            (sub, CarriedBy::Request(number, end))
                        }
                        false => {
                            let by = CarriedBy::Entry(sub);
                            (own_end(s, own.clone(), Some(&by))?, by)
                        }
                    };
                    let names = &names;
                    let by = Some(&by);
                    return report_delivering(
                        s,
                        delivered_to,
                        end,
                        branch,
                        names,
                        by,
                        copy.as_ref(),
                        json,
                        confirms,
                        out,
                    );
                }
                match &own {
                    Some(own) if takes_own && own.id != sub.id => {
                        return await_own(own, copy.as_ref(), out);
                    }
                    _ => {}
                }
                // 51: the request was opened from this worktree's branch,
                // so the outcome a waiting submit reports is its own; a
                // branch named's is no one's (124).
                return report_delivering(
                    s,
                    delivered_to,
                    sub,
                    branch,
                    &names,
                    None,
                    copy.as_ref(),
                    json,
                    confirms,
                    out,
                );
            }
            // 124, 137: `--status` says where it stands, and exits 0, as
            // it does where this host takes the request in; a wait and a
            // waiting submit report the outcome with its exit (record 0096).
            Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) if status => {
                return across_status(s, worktree, number, branch, &f, copy.as_ref(), json, out);
            }
            // Record 0100 D2: a wait that follows the request for its own
            // ends with its own, landed where the branch holds its head.
            Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. } | Followed::Closed))
                if follows_for.is_some() =>
            {
                let own = follows_for.expect("the submission it follows for");
                let end = match f {
                    Followed::Landed { .. } => RequestEnd::Landed,
                    Followed::SentBack { conflict: true, .. } => RequestEnd::Conflict,
                    Followed::SentBack { .. } => RequestEnd::Failed,
                    _ => RequestEnd::Closed,
                };
                let by = CarriedBy::Request(number, end);
                let own = own_end(s, own.clone(), Some(&by))?;
                let names = BTreeMap::new();
                return report_delivering(
                    s,
                    delivered_to,
                    own,
                    branch,
                    &names,
                    Some(&by),
                    copy.as_ref(),
                    json,
                    confirms,
                    out,
                );
            }
            Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) => {
                return across(number, branch, &f, copy.as_ref(), json, out);
            }
            Some(Followed::Closed) => {
                if json {
                    let doc = serde_json::json!({
                        "pull_request": number, "branch": branch, "state": "closed",
                    });
                    writeln!(out, "{}", with_copy(doc, copy.as_ref())?)?;
                } else {
                    writeln!(out, "#{number} was closed without landing")?;
                }
                return Ok(FAILED);
            }
            Some(Followed::Open {
                title,
                lines,
                taken_in_by,
                queue,
                submission,
                names,
                head,
                draft,
            }) => {
                // 137, design D5: where another host takes it in, the host
                // the mark names and the queue its check carries, said
                // again only when it changes.
                there = pulls
                    .elsewhere(branch)
                    .map(|_| There { taken_in_by, queue });
                if let (false, Some(t)) = (json, &there) {
                    let line = t.line(number, branch);
                    if said_there.as_ref() != Some(&line) {
                        writeln!(out, "{line}")?;
                        said_there = Some(line);
                    }
                }
                // Taken in on the head just pushed: the submission is the
                // answer, as a no-wait submit's on any branch, with the
                // pull request beside it.
                if let (true, Some(sub)) = (no_wait, &submission) {
                    let ahead: Vec<u64> = s
                        .status(branch)?
                        .submissions
                        .iter()
                        .filter(|x| x.id < sub.id && !x.state.is_out())
                        .map(|x| x.id)
                        .collect();
                    if json {
                        let doc = serde_json::json!({
                            "id": sub.id, "branch": branch, "ahead": ahead,
                            "pull_request": number,
                        });
                        writeln!(out, "{}", with_copy(doc, copy.as_ref())?)?;
                    } else {
                        let name =
                            |i: u64| names.get(&i).cloned().unwrap_or_else(|| format!("#{i}"));
                        let ahead = queued_ahead(&ahead, name);
                        writeln!(
                            out,
                            "submission {} to {branch} from #{number} · {ahead}",
                            sub.id
                        )?;
                    }
                    return Ok(0);
                }
                // Record 0100 D4: its own is the first submission of the
                // request it reads; taken in as another, it waits on its own.
                if let (true, Some(e)) = (takes_own, &submission) {
                    match &own {
                        None => own = Some(e.clone()),
                        Some(own) if own.id != e.id => return await_own(own, copy.as_ref(), out),
                        Some(_) => {}
                    }
                }
                let now = (title, lines);
                if last.as_ref() != Some(&now) {
                    if json && status {
                        // Its head and draft mark, for a driver to tell
                        // whether it stands at what the worktree would send
                        // (record 0065).
                        let doc = serde_json::json!({
                            "pull_request": number, "branch": branch, "state": now.0,
                            "head": head, "draft": draft,
                        });
                        let doc = with_sends(There::with(doc, there.as_ref())?, s, worktree)?;
                        writeln!(out, "{}", with_copy(doc, copy.as_ref())?)?;
                    } else if json && no_wait {
                        let doc = serde_json::json!({
                            "pull_request": number, "branch": branch, "state": now.0,
                        });
                        let doc = There::with(doc, there.as_ref())?;
                        writeln!(out, "{}", with_copy(doc, copy.as_ref())?)?;
                    } else if !json {
                        writeln!(out, "{}", now.0)?;
                        for l in &now.1 {
                            writeln!(out, "  {l}")?;
                        }
                    }
                    last = Some(now);
                }
                taken = submission.map(|x| x.id);
            }
            None => {}
        }
        if no_wait || status {
            return Ok(0);
        }
        // Undecided: taken in, its submission's record as it stands now, as
        // `sluice wait <id>` ends; otherwise the document `--status`
        // prints, or where to read it again.
        let still = |out: &mut dyn Write, code: i32| -> Result<i32> {
            let sub = match taken {
                Some(id) => s.state_on(branch, id)?,
                None => None,
            };
            // Record 0100 D2: a wait that follows the request for its own
            // ends with its own record, and what carries its work as it
            // stands.
            if let Some(own) = follows_for {
                let by = match sub {
                    Some(e) => CarriedBy::Entry(e),
                    None => CarriedBy::Request(number, RequestEnd::Open),
                };
                if json {
                    let doc = with_copy(serde_json::to_value(own)?, copy.as_ref())?;
                    let doc = with_carried(doc, Some(&by));
                    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                } else {
                    writeln!(out, "still waiting: sluice wait {}", own.id)?;
                }
                return Ok(code);
            }
            if let Some(sub) = sub {
                if json {
                    writeln!(out, "{}", record_json(&sub, copy.as_ref())?)?;
                } else {
                    writeln!(out, "still waiting: sluice wait {}", sub.id)?;
                }
            } else if json {
                let state = last.as_ref().map_or("waiting", |l| l.0.as_str());
                let doc = serde_json::json!({
                    "pull_request": number, "branch": branch, "state": state,
                });
                let doc = There::with(doc, there.as_ref())?;
                writeln!(out, "{}", with_copy(doc, copy.as_ref())?)?;
            } else {
                writeln!(out, "still open: #{number}; sluice wait {branch}")?;
            }
            Ok(code)
        };
        if INTERRUPTED.load(Ordering::SeqCst) {
            return still(out, STOPPED);
        }
        if let Some(d) = mode.deadline() {
            if clock.now() >= d {
                return still(out, WAITING);
            }
        }
        // 137: no round where another host takes the branch's pull
        // requests in.
        let away = pulls.elsewhere(branch).is_some();
        let round = if driven || away {
            None
        } else {
            Some(s.round(branch))
        };
        match round {
            None => {}
            Some(Ok(round)) => {
                exec::tell_abandoned(out, json, &round.released)?;
                exec::tell_releases(out, json, &round.releases)?;
                if !json && matches!(round.step, Step::Stepped | Step::Held) {
                    tell_hold(out, branch, round.held.as_ref(), &mut said_hold)?;
                }
            }
            Some(Err(e)) => {
                if INTERRUPTED.load(Ordering::SeqCst) {
                    return still(out, STOPPED);
                }
                // 122: as `wait_on`, for the submission the pull request
                // was taken in as, on the head its worktree, or the branch
                // named, submitted (124).
                let submitted = match head {
                    SubmitHead::Worktree(w) => seams
                        .driver
                        .session(w)
                        .ok()
                        .and_then(|w| s.submitted_head(&w).ok()),
                    SubmitHead::Branch(b) => s
                        .named_session(b, branch)
                        .ok()
                        .and_then(|r| r.ok())
                        .map(|(_, h)| h),
                };
                let landed = submitted
                    .and_then(|h| taken_in(s, branch, &h.head))
                    .and_then(|id| landed(s, id, branch));
                if let Some(sub) = landed {
                    let names = match pulls.follow(seams, clock, branch, number) {
                        Ok(Followed::Done(_, names)) => names,
                        _ => BTreeMap::new(),
                    };
                    // Record 0100 D2: another submission's landing is its
                    // own's end, never its own.
                    let (sub, by) = match follows_for.or(own.as_ref()) {
                        Some(own) if own.id != sub.id => {
                            let by = CarriedBy::Entry(sub);
                            (own_end(s, own.clone(), Some(&by))?, Some(by))
                        }
                        _ => (sub, None),
                    };
                    let code = report_delivering(
                        s,
                        delivered_to,
                        sub,
                        branch,
                        &names,
                        by.as_ref(),
                        copy.as_ref(),
                        json,
                        confirms,
                        out,
                    )?;
                    after_landing(&e);
                    return Ok(code);
                }
                return could_not_read(&unread(e)?, &what, &again, json, out);
            }
        }
        if !pause.pause() {
            return still(out, STOPPED);
        }
    }
}

/// 125: `sluice submit` of several worktrees to a branch with pull
/// requests: every worktree's branch pushed and its pull request opened or
/// updated, each marked one of the group, or none when any worktree is
/// refused; then every pull request marked ready and queued, each
/// submission said after its pull request, or none when any falls short
/// of its conditions, each such named. It says where each stands; then,
/// unless `no_wait`, it follows each, taking pull requests in and running
/// rounds as a waiting pull-request submit does, or leaving both to
/// `sluice run` where it lives on the host (135), until every one is
/// decided, and reports each one's outcome in the order given.
#[allow(clippy::too_many_arguments)]
fn submit_pull_group(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    req: GroupRequest,
    no_wait: bool,
    json: bool,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    let seams = s.seams();
    let branch = &req.branch;
    let names = req.names();
    let opened = match pulls.open_group(seams, clock, &names, &req.subjects, branch)? {
        Ok(o) => o,
        Err(GroupRefused::Worktrees(refused)) => {
            return refused_group(s, &req, &refused, &(), json, out)
        }
        Err(GroupRefused::Short { opened, short }) => {
            return short_group(&req, &opened, &short, json, out)
        }
    };
    if !json {
        // What is queued ahead is named as on a pull request: the act's
        // own by their pull requests, the rest as intake last read them.
        let ours: BTreeMap<u64, u64> = opened
            .iter()
            .filter_map(|o| Some((o.submission.as_ref()?.id, o.number)))
            .collect();
        let others = opened
            .iter()
            .filter_map(|o| o.submission.as_ref())
            .any(|a| a.ahead.iter().any(|i| !ours.contains_key(i)));
        let names = match others.then(|| pulls.follow(seams, clock, branch, opened[0].number)) {
            Some(Ok(Followed::Open { names, .. } | Followed::Done(_, names))) => names,
            _ => BTreeMap::new(),
        };
        let name = |i: u64| match ours.get(&i) {
            Some(n) => format!("#{n}"),
            None => names.get(&i).cloned().unwrap_or_else(|| format!("#{i}")),
        };
        for o in &opened {
            writeln!(out, "{}", o.said)?;
            if let Some(a) = &o.submission {
                let ahead = queued_ahead(&a.ahead, name);
                writeln!(
                    out,
                    "submission {} to {branch} from #{} · {ahead}",
                    a.id, o.number
                )?;
            }
        }
    }
    let mut decided: Vec<Option<Followed>> = opened.iter().map(|_| None).collect();
    let mut last: Vec<Option<(String, Vec<String>)>> = vec![None; opened.len()];
    // 137: where each stands on the host that takes it in, as last read
    // and as last said, where that is another host.
    let mut there: Vec<Option<There>> = vec![None; opened.len()];
    let mut said_there: Vec<Option<String>> = vec![None; opened.len()];
    // 121: the branch's hold as last said, so each is said once.
    let mut said_hold = None;
    // As `submit_pull`: a read the forge could not answer for the retry
    // policy's time ends the wait, said on stderr.
    let what = hashes(
        &opened.iter().map(|o| o.number).collect::<Vec<_>>(),
        None,
        &mut BTreeSet::new(),
    );
    let again = format!("sluice wait {branch}");
    // 122: what failed after a landing, said once the outcomes are.
    let mut after = Vec::new();
    let probe = driven_by(host);
    let mut first = true;
    // 137: a wait lets go of the intake mark it holds when it ends.
    let _let_go = LetGo(Some(pulls));
    let stopped = 'wait: loop {
        // 135: as `submit_pull`, the first pass's intake the submit's own.
        let driven = probe();
        if !driven || first {
            let taker = match driven || no_wait {
                true => Taker::Step,
                false => Taker::Wait,
            };
            match intake(pulls, seams, clock, branch, taker, json, out) {
                Some(e) if no_wait => return Err(e),
                Some(_) if INTERRUPTED.load(Ordering::SeqCst) => break 'wait Some(STOPPED),
                _ => {}
            }
        }
        first = false;
        for (i, o) in opened.iter().enumerate() {
            if decided[i].is_some() {
                continue;
            }
            let followed = match pulls.follow(seams, clock, branch, o.number) {
                Ok(f) => f,
                Err(e) if no_wait => return Err(e),
                // Ctrl-C ended the retry policy's pauses.
                Err(_) if INTERRUPTED.load(Ordering::SeqCst) => break 'wait Some(STOPPED),
                Err(e) => {
                    let e = unread(e)?;
                    let code = could_not_read(&e, &what, &again, false, &mut std::io::sink())?;
                    break 'wait Some(code);
                }
            };
            match followed {
                Followed::Open {
                    title,
                    lines,
                    taken_in_by,
                    queue,
                    ..
                } => {
                    // 137, design D5: as `submit_pull`'s.
                    there[i] = pulls
                        .elsewhere(branch)
                        .map(|_| There { taken_in_by, queue });
                    if let (false, Some(t)) = (json, &there[i]) {
                        let line = t.line(o.number, branch);
                        if said_there[i].as_ref() != Some(&line) {
                            writeln!(out, "{line}")?;
                            said_there[i] = Some(line);
                        }
                    }
                    let now = (title, lines);
                    if last[i].as_ref() != Some(&now) {
                        if !json {
                            writeln!(out, "#{} {}", o.number, now.0)?;
                            for l in &now.1 {
                                writeln!(out, "  {l}")?;
                            }
                        }
                        last[i] = Some(now);
                    }
                }
                followed => decided[i] = Some(followed),
            }
        }
        if no_wait || decided.iter().all(Option::is_some) {
            break None;
        }
        let away = pulls.elsewhere(branch).is_some();
        let round = if driven || away {
            None
        } else {
            Some(s.round(branch))
        };
        match round {
            None => {}
            Some(Ok(round)) => {
                exec::tell_abandoned(out, json, &round.released)?;
                exec::tell_releases(out, json, &round.releases)?;
                if !json && matches!(round.step, Step::Stepped | Step::Held) {
                    tell_hold(out, branch, round.held.as_ref(), &mut said_hold)?;
                }
            }
            Some(Err(e)) => {
                if INTERRUPTED.load(Ordering::SeqCst) {
                    break Some(STOPPED);
                }
                // 122: as `submit_pull`, each the round landed is decided,
                // on the head its worktree submitted (124), and what failed
                // is said after the outcomes.
                let mut landed_now = false;
                for (i, (w, o)) in names.iter().zip(&opened).enumerate() {
                    if decided[i].is_some() {
                        continue;
                    }
                    let sub = seams
                        .driver
                        .session(w)
                        .ok()
                        .and_then(|w| s.submitted_head(&w).ok())
                        .and_then(|h| taken_in(s, branch, &h.head))
                        .and_then(|id| landed(s, id, branch));
                    if let Some(sub) = sub {
                        let names = match pulls.follow(seams, clock, branch, o.number) {
                            Ok(Followed::Done(_, names)) => names,
                            _ => BTreeMap::new(),
                        };
                        decided[i] = Some(Followed::Done(sub, names));
                        landed_now = true;
                    }
                }
                if landed_now {
                    after.push(e);
                    if decided.iter().all(Option::is_some) {
                        break None;
                    }
                } else {
                    let e = unread(e)?;
                    let code = could_not_read(&e, &what, &again, false, &mut std::io::sink())?;
                    break Some(code);
                }
            }
        }
        if !pause.pause() {
            break Some(STOPPED);
        }
    };
    let state = |i: usize| last[i].as_ref().map_or("waiting", |l| l.0.as_str());
    if json {
        let mut items = Vec::new();
        for (i, (w, o)) in names.iter().zip(&opened).enumerate() {
            let mut item = match &decided[i] {
                Some(Followed::Done(sub, _)) => serde_json::to_value(sub)?,
                Some(Followed::Closed) => serde_json::json!({ "state": "closed" }),
                Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) => across_json(f),
                _ => There::with(serde_json::json!({ "state": state(i) }), there[i].as_ref())?,
            };
            item["worktree"] = serde_json::json!(w);
            item["pull_request"] = serde_json::json!(o.number);
            if let (true, Some(a)) = (no_wait, &o.submission) {
                item["id"] = serde_json::json!(a.id);
                item["ahead"] = serde_json::json!(a.ahead);
            }
            items.push(with_copy(item, o.copy.as_ref())?);
        }
        let doc = serde_json::json!({ "branch": branch, "submissions": items });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    }
    if no_wait {
        return Ok(0);
    }
    // Each one's outcome in the order given; the exit is the first's, in
    // that order, that did not land.
    let mut code = LANDED;
    for (i, (w, o)) in names.iter().zip(&opened).enumerate() {
        let n = o.number;
        if !json {
            writeln!(out, "#{n} from {w}:")?;
        }
        let this = match &decided[i] {
            Some(Followed::Done(sub, _)) if json => exit_for(sub),
            Some(Followed::Done(sub, names)) => {
                report(s, sub, branch, names, None, None, None, false, out)?
            }
            Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) if json => {
                across_exit(f)
            }
            Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) => {
                across_lines(f, out)?;
                across_exit(f)
            }
            Some(Followed::Closed) => {
                if !json {
                    writeln!(out, "#{n} was closed without landing")?;
                }
                FAILED
            }
            _ => {
                if !json {
                    writeln!(out, "still open: #{n}; sluice wait {branch}")?;
                }
                LANDED
            }
        };
        if code == LANDED {
            code = this;
        }
    }
    for e in &after {
        after_landing(e);
    }
    Ok(stopped.unwrap_or(code))
}

/// 137, design D2: where a pull request another host takes in stands, as
/// a wait last read it: the host its branch's intake mark names, as this
/// host's last fetch brought it, and the queue sluice's check on its head
/// carries.
#[derive(Clone, Debug, PartialEq)]
struct There {
    taken_in_by: Option<TakenInBy>,
    queue: Option<Queue>,
}

impl There {
    /// Design D5: pull request `n` into `branch` in words: the submission
    /// the check names on the host that posted it, taken in by the host the
    /// mark names; or that it waits to be taken in by the mark's host.
    fn line(&self, n: u64, branch: &str) -> String {
        let held = self
            .queue
            .as_ref()
            .filter(|q| q.taken_in)
            .and_then(|q| Some((q.submission?, &q.host)));
        let by = self.taken_in_by.as_ref().map(TakenInBy::said);
        let none = format!("no host holds {branch}'s intake mark");
        match (held, by) {
            (Some((id, host)), Some(by)) => {
                format!("#{n} is submission {id} on {host}, taken in by {by}")
            }
            (Some((id, host)), None) => format!("#{n} is submission {id} on {host}; {none}"),
            (None, Some(by)) => format!("#{n} waits to be taken in by {by}"),
            (None, None) => format!("#{n} waits to be taken in; {none}"),
        }
    }

    /// Design D5: `doc` with `taken_in_by` and `queue`, `null` where
    /// neither reads, for a pull request another host takes in; `doc` as
    /// it is otherwise.
    fn with(mut doc: serde_json::Value, there: Option<&There>) -> Result<serde_json::Value> {
        if let Some(t) = there {
            doc["taken_in_by"] = serde_json::to_value(&t.taken_in_by)?;
            doc["queue"] = serde_json::to_value(&t.queue)?;
        }
        Ok(doc)
    }
}

/// 137: the outcome GitHub shows of pull request `number`, which another
/// host took in: said with the words a submission's report gives for the
/// same outcome, as sluice's check on its head names it, and its exit.
fn across(
    number: u64,
    branch: &str,
    f: &Followed,
    copy: Option<&CopyDiffers>,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    if json {
        let mut doc = across_json(f);
        doc["pull_request"] = serde_json::json!(number);
        doc["branch"] = serde_json::json!(branch);
        writeln!(out, "{}", with_copy(doc, copy)?)?;
    } else {
        across_lines(f, out)?;
    }
    Ok(across_exit(f))
}

/// 124, 137: `--status` of pull request `number`, which another host
/// landed or sent back: under `json`, [`across_json`]'s document with the
/// check's summary, the head and draft mark the forge holds, and `sends`,
/// the revision a submit from `worktree` would send now, so a driver
/// reads it as it reads a decided submission's record, landed standing at
/// `sends` and sent back standing nowhere; otherwise the check's title and
/// summary in words. It exits 0, as `--status` does wherever it finds the
/// worktree's pull request (record 0096).
#[allow(clippy::too_many_arguments)]
fn across_status(
    s: &Sluice<'_>,
    worktree: &str,
    number: u64,
    branch: &str,
    f: &Followed,
    copy: Option<&CopyDiffers>,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let (summary, head, draft) = match f {
        Followed::Landed {
            summary,
            head,
            draft,
            ..
        }
        | Followed::SentBack {
            summary,
            head,
            draft,
            ..
        } => (summary, head, *draft),
        _ => return Ok(0),
    };
    if json {
        let mut doc = across_json(f);
        doc["summary"] = serde_json::json!(summary);
        doc["pull_request"] = serde_json::json!(number);
        doc["branch"] = serde_json::json!(branch);
        doc["head"] = serde_json::json!(head);
        doc["draft"] = serde_json::json!(draft);
        let doc = with_sends(doc, s, worktree)?;
        writeln!(out, "{}", with_copy(doc, copy)?)?;
        return Ok(0);
    }
    across_lines(f, out)?;
    if let Followed::Landed { summary, .. } = f {
        for l in summary.lines().filter(|l| !l.trim().is_empty()) {
            writeln!(out, "  {l}")?;
        }
    }
    Ok(0)
}

/// 137: [`across`]'s words: `passed · landed as …` for a landing, and
/// sluice's check's title and summary for a pull request sent back.
fn across_lines(f: &Followed, out: &mut dyn Write) -> Result<()> {
    match f {
        Followed::Landed { title, .. } => writeln!(out, "passed · {title}")?,
        Followed::SentBack { title, summary, .. } => {
            writeln!(out, "{title}")?;
            for l in summary.lines().filter(|l| !l.trim().is_empty()) {
                writeln!(out, "  {l}")?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 137: [`across`]'s document: the state, and sluice's check's title.
fn across_json(f: &Followed) -> serde_json::Value {
    match f {
        Followed::Landed { title, .. } => serde_json::json!({ "state": "landed", "title": title }),
        Followed::SentBack {
            conflict,
            title,
            summary,
            ..
        } => serde_json::json!({
            "state": if *conflict { "conflict" } else { "failed" },
            "title": title,
            "summary": summary,
        }),
        _ => serde_json::json!({}),
    }
}

/// 137: [`across`]'s exit, a submission's for the same outcome.
fn across_exit(f: &Followed) -> i32 {
    match f {
        Followed::Landed { .. } => LANDED,
        Followed::SentBack { conflict: true, .. } => CONFLICT,
        _ => FAILED,
    }
}

/// The records seam of a sluice, for reading the log.
fn s_state<'a>(s: &'a Sluice<'a>) -> &'a dyn sluice_seams::State {
    s.seams().state
}

fn exit_for(sub: &Submission) -> i32 {
    match sub.state {
        State::Landed => LANDED,
        State::Conflict => CONFLICT,
        State::Withdrawn => WITHDRAWN,
        _ => FAILED,
    }
}

/// A failed task as a person reads it: its system where the policy names
/// one (85), and `(flaky)` when its key holds both outcomes (19).
fn failed_name(f: &sluice_core::records::FailedTask) -> String {
    let mut name = f.task.clone();
    if let Some(s) = &f.system {
        name.push_str(&format!(" ({s})"));
    }
    if f.flaky.is_some() {
        name.push_str(" (flaky)");
    } else if f.not_run.is_some() {
        name.push_str(" (could not run)");
    }
    name
}

/// What a row says of `s` after its columns, naming the submissions it
/// names as `names` and `said` do.
fn detail(branch: &str, s: &Submission, names: Option<&Names>, said: &mut BTreeSet<u64>) -> String {
    let shown = detail_of_state(branch, s, names, said);
    // 140: what work no check reads at the branch's revision waits behind,
    // until it lands.
    match &s.beside {
        Some(Beside::Behind { behind, why }) if !s.state.is_out() => {
            let waits = words::waits_beside(*behind, *why);
            match shown.is_empty() {
                true => waits,
                false => format!("{shown} · {waits}"),
            }
        }
        _ => shown,
    }
}

/// What `detail` says of a submission in its state alone.
fn detail_of_state(
    branch: &str,
    s: &Submission,
    names: Option<&Names>,
    said: &mut BTreeSet<u64>,
) -> String {
    match s.state {
        State::Failed => {
            // 19: as a waiting submit names it.
            let tasks: Vec<String> = s.failed.iter().map(failed_name).collect();
            let what = if tasks.is_empty() {
                s.result.clone().unwrap_or_default()
            } else {
                tasks.join(", ")
            };
            match &s.tested {
                Some(t) => format!("{what} on {}", t.combination.short()),
                None => what,
            }
        }
        State::Testing | State::Passed => {
            let tested = s
                .tested
                .as_ref()
                .map(|t| combination(branch, &t.submissions, names, said))
                .unwrap_or_default();
            // 117: what a held submission waits for, and until when.
            match &s.held {
                Some(h) => format!(
                    "{tested}, held for {} until {}",
                    combination(branch, &h.by, names, said),
                    h.until
                ),
                None => tested,
            }
        }
        State::Conflict => s
            .conflict
            .as_ref()
            .map(|c| {
                let ids: Vec<u64> = c.with.iter().map(|w| w.id).collect();
                format!("{} with {}", c.paths.join(", "), hashes(&ids, names, said))
            })
            .unwrap_or_default(),
        State::Waiting => format!("after {}", hashes(&s.after, names, said)),
        // 120, 137: withdrawn at a hand-over of intake, which holds no
        // request: the host it went to.
        State::Withdrawn => s
            .withdrawn
            .as_ref()
            .and_then(|w| w.handed_over_to.as_deref())
            .map(|host| format!("handed over to {host}"))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Each submission a branch's status may name (26): those in its live
/// records, those moved to its finished records, and the rows it shows,
/// which carry their own.
fn status_names(status: &Status, subs: &[Submission]) -> Names {
    let mut names = Names::default();
    for (id, name) in &status.named {
        names.insert(*id, name);
    }
    for s in status.submissions.iter().chain(subs) {
        names.insert(s.id, s.name());
    }
    names
}

/// A branch's status as text: its counts, where it is pushed, its
/// warnings, `testing`, a line per combination under test that is to be
/// shown (ADR 0042), and a row per submission in `subs`, each beginning
/// with its number and its name (26).
fn write_status(
    status: &Status,
    pusher: Option<&Pusher>,
    testing: &[String],
    subs: &[Submission],
    names: &Names,
    room: Option<&sluice_core::overview::Room>,
    out: &mut dyn Write,
) -> Result<()> {
    let at = status.at.as_ref().map_or("nothing", |r| r.short());
    let up_to = status
        .parallel
        .map_or(String::new(), |p| format!(" (up to {p})"));
    let queued = status
        .submissions
        .iter()
        .filter(|s| s.state == sluice_core::records::State::Queued)
        .count();
    let held = overview::held_clause(room, queued, status.testing, status.window);
    if status.gone {
        // 105: its records stay and are read; nothing is formed on it.
        writeln!(
            out,
            "{}",
            overview::gone_line(&status.branch, status.at.as_ref())
        )?;
    } else {
        writeln!(
            out,
            "{} at {at} · {} in the queue · testing {} at once{up_to}{held}",
            status.branch,
            status.depth(),
            status.testing
        )?;
    }
    match pusher {
        None => {}
        Some(Pusher {
            remote,
            identity: Some(who),
        }) => writeln!(out, "pushed to {remote} as {who}")?,
        Some(Pusher {
            remote,
            identity: None,
        }) => writeln!(
            out,
            "pushed to {remote} with this host's own git credentials: \
             sluice has no identity of its own there"
        )?,
    }
    for w in &status.warnings {
        writeln!(out, "warning: {w}")?;
    }
    if let Some(remote) = &status.stale {
        writeln!(
            out,
            "{remote} could not be reached: this is the state last fetched from it"
        )?;
    }
    if let Some(hold) = &status.held {
        writeln!(out, "{}", held_line(&status.branch, hold))?;
    }
    write_failed_releases(&status.failed_releases, out)?;
    if let Some(copy) = &status.copy {
        writeln!(out, "{}", copy_behind_line(&status.branch, copy))?;
    }
    if let Some(refused) = &status.refused {
        writeln!(out, "{}", refused_lines(refused))?;
    }
    write_refused_writes(status, out)?;
    for t in testing {
        writeln!(out, "  {t}")?;
    }
    let aw = subs.iter().map(|s| s.from.agent.len()).max().unwrap_or(0);
    let ww = subs
        .iter()
        .map(|s| s.from.worktree.len())
        .max()
        .unwrap_or(0);
    let iw = subs
        .iter()
        .map(|s| s.id.to_string().len())
        .max()
        .unwrap_or(0);
    let name = |s: &Submission| names.get(s.id).unwrap_or_default();
    let nw = subs
        .iter()
        .map(|s| name(s).chars().count())
        .max()
        .unwrap_or(0);
    for s in subs {
        // A row's own number stands alone in its detail.
        let mut said = BTreeSet::from([s.id]);
        let line = format!(
            "  #{:<iw$} {:<nw$}  {:<8}  {:<aw$}  {:<ww$}   {}",
            s.id,
            name(s),
            s.state.word(),
            s.from.agent,
            s.from.worktree,
            detail(&status.branch, s, Some(names), &mut said),
        );
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}

/// 51, D13: a branch's `owed`: each reader owed something, whether any
/// reader has confirmed under its name, and its deliveries.
fn owed_json(owed: &[sluice_core::ReaderView]) -> serde_json::Value {
    owed.iter()
        .map(|r| {
            let deliveries: Vec<_> = r
                .deliveries
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "key": d.key,
                        "id": d.id,
                        "state": if d.expired { "expired" } else { "owed" },
                        "expires_at": d.expires_at,
                        "handed": d
                            .handed
                            .iter()
                            .map(|(by, at)| serde_json::json!({ "by": by, "at": at }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            serde_json::json!({
                "reader": r.reader,
                "known": r.known,
                "deliveries": deliveries,
            })
        })
        .collect()
}

/// 51, D13 (weir 104 to 106; S77): the branch's `OWED` section, after its
/// queue: each reader owed something, marked `never confirmed` where no
/// reader has confirmed under its name, and under it each delivery by key
/// and submission, owed until or expired at its expiry, with each
/// hand-over's way and time; nothing where nothing is owed.
fn write_owed(owed: &[sluice_core::ReaderView], out: &mut dyn Write) -> Result<()> {
    if owed.is_empty() {
        return Ok(());
    }
    writeln!(out, "OWED")?;
    for r in owed {
        match r.known {
            true => writeln!(out, "  {}", r.reader)?,
            false => writeln!(out, "  {} · never confirmed", r.reader)?,
        }
        for d in &r.deliveries {
            let id = d.id.map_or(String::new(), |id| format!(" #{id}"));
            let when = match d.expired {
                true => format!("expired at {}", d.expires_at),
                false => format!("owed until {}", d.expires_at),
            };
            let handed = match d.handed.as_slice() {
                [] => "handed over by nothing".to_string(),
                handed => {
                    let each: Vec<String> = handed
                        .iter()
                        .map(|(by, at)| format!("{by} at {at}"))
                        .collect();
                    format!("handed over by {}", each.join(", "))
                }
            };
            writeln!(out, "    {}{id}  {when} · {handed}", d.key)?;
        }
    }
    writeln!(
        out,
        "confirm one with: sluice confirm <key>, in its worktree, or sluice confirm <key> --as operator"
    )?;
    Ok(())
}

/// 123: the branch's checkouts whose release failed, each with how many
/// tries and what failed, and how to release one or give up on it.
fn write_failed_releases(
    failed: &[sluice_core::records::FailedRelease],
    out: &mut dyn Write,
) -> Result<()> {
    if failed.is_empty() {
        return Ok(());
    }
    writeln!(out, "held checkouts, whose release failed:")?;
    let w = failed.iter().map(|f| f.place.len()).max().unwrap_or(0);
    for f in failed {
        let tries = match f.tries {
            1 => "1 try".to_string(),
            n => format!("{n} tries"),
        };
        let now = match f.stopped {
            true => "held",
            false => "tried again by the next round",
        };
        let host = f
            .host
            .as_ref()
            .map_or(String::new(), |h| format!(", on {h}"));
        writeln!(
            out,
            "  {:<w$}  {tries}, {now}{host}, last at {}",
            f.place, f.at
        )?;
        for line in f.failed.trim_end().lines() {
            writeln!(out, "    | {line}")?;
        }
    }
    writeln!(
        out,
        "release one with: sluice release <checkout>, or without its release command: \
         sluice release <checkout> --accept-failure"
    )?;
    Ok(())
}

fn status_json(status: &Status, submissions: &[Submission]) -> Result<serde_json::Value> {
    let submissions = submissions
        .iter()
        .map(|sub| decided_document(sub, None))
        .collect::<Result<Vec<_>>>()?;
    let held_checkouts: Vec<serde_json::Value> = status
        .failed_releases
        .iter()
        .map(|f| {
            serde_json::json!({
                "place": f.place,
                "failed": f.failed,
                "tries": f.tries,
                "at": f.at,
                "stopped": f.stopped,
                "host": f.host,
            })
        })
        .collect();
    let mut json = serde_json::json!({
        "branch": status.branch,
        "at": status.at,
        "gone": status.gone,
        "depth": status.depth(),
        "testing": status.testing,
        "window": status.window,
        "parallel": status.parallel,
        "submissions": submissions,
        "warnings": status.warnings,
        "stale": status.stale,
        "held": status.held.as_ref().map(held_json),
        "held_checkouts": held_checkouts,
        "copy": status.copy,
        "refused_writes": status.refused_writes,
    });
    if let Some(refused) = &status.refused {
        json["refused"] = serde_json::json!(refused);
    }
    Ok(json)
}

/// The names of a combination's checkout begin so: `sluice/try/<branch>/<n>`.
const CHECKOUT: &str = "sluice/try/";

/// 123: `sluice release sluice/try/<branch>/<n>`: the held checkout of
/// that name on this host, its release run once more under its branch's
/// lock, or with `accept` freed with nothing run. Exits 1, saying what
/// failed, when its release fails again.
fn release_checkout(
    s: &Sluice<'_>,
    name: &str,
    accept: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    use sluice_core::release::Releasing;
    let branch = name
        .strip_prefix(CHECKOUT)
        .and_then(|rest| rest.rsplit_once('/'))
        .filter(|(b, n)| !b.is_empty() && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        .map(|(b, _)| b);
    let outcome = match branch {
        Some(b) => {
            let _lock = s.seams().state.wait_lock(b)?;
            s.release_held(b, name, accept)?
        }
        None => None,
    };
    let how = outcome.map(|o| o.how);
    let failed = match &how {
        Some(Releasing::Failed { failed, .. } | Releasing::Held { failed, .. }) => {
            Some(failed.clone())
        }
        _ => None,
    };
    let text = match &how {
        None => "nothing to release".to_string(),
        Some(Releasing::Released) => {
            format!("released {name}: the release command run, its checkout removed, its ports free")
        }
        Some(Releasing::Gone { ran, failed }) => format!(
            "freed {name}: it no longer exists; {}; its runs ended, its ports free",
            exec::gone_release(*ran, failed.as_deref())
        ),
        Some(Releasing::Accepted(_)) => format!(
            "freed {name} without its release command: its runs ended, its checkout removed, its ports free"
        ),
        Some(Releasing::Failed { failed, .. } | Releasing::Held { failed, .. }) => format!(
            "could not release {name}: {failed}\n\
             `sluice release {name}` runs it again; `sluice release {name} --accept-failure` frees it without"
        ),
    };
    if json {
        writeln!(std::io::stderr(), "{text}")?;
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "place": name,
                "released": how.as_ref().is_some_and(Releasing::freed),
                "accepted": matches!(how, Some(Releasing::Accepted(_))),
                "failed": failed,
            })
        )?;
    } else {
        writeln!(out, "{text}")?;
    }
    Ok(if failed.is_some() { FAILED } else { LANDED })
}

/// 124, S58: the worktree's head, this host's copy of a branch sluice
/// lands on, where it differs from the remote's revision submitted.
pub(crate) fn copy_differs_line(copy: &CopyDiffers) -> String {
    format!(
        "this host's {} is at {}, {} {}'s {}; submitted {}",
        copy.branch,
        copy.copy.short(),
        copy.how.words(),
        copy.remote,
        copy.at.short(),
        copy.at.short()
    )
}

/// 133: this host's refusal of the branch's remote, git's words on as
/// many lines as git gave them.
fn refused_lines(refused: &sluice_core::Refused) -> String {
    let rounds = match refused.rounds {
        1 => "1 round".to_string(),
        n => format!("{n} rounds"),
    };
    let mut said = refused.said.lines();
    let mut text = format!(
        "this host could not read {} from {} since {}, {rounds}: {}",
        refused.asked,
        refused.remote,
        refused.since,
        said.next().unwrap_or_default()
    );
    for line in said {
        text.push_str(&format!("\n  {line}"));
    }
    text
}

/// 148: the writes sluice's protection refused on the branch whose causes
/// stand, newest first, each by its command and then by its host, count,
/// first, last and last revisions; then what to do, in the words of the
/// copy's refusal where the branch is kept on a remote and of the
/// refusal itself where it is not (124; design D5).
fn write_refused_writes(status: &Status, out: &mut dyn Write) -> Result<()> {
    if status.refused_writes.is_empty() {
        return Ok(());
    }
    let branch = &status.branch;
    writeln!(
        out,
        "sluice refused writes to {branch} in the last day, by commands that are not sluice's:"
    )?;
    for c in &status.refused_writes {
        let times = match c.times {
            1 => "1 time".to_string(),
            n => format!("{n} times"),
        };
        let to = match &c.to {
            Some(to) => format!(" to {}", to.short()),
            None => ", deleting it".to_string(),
        };
        writeln!(out, "  {}", overview::command_line(&c.command))?;
        writeln!(
            out,
            "    on {}: {times}, first {}, last {}, from {}{to}",
            c.host,
            c.first,
            c.last,
            c.from.short()
        )?;
    }
    if status.on_remote {
        writeln!(
            out,
            "sluice sets each host's {branch} to each landing itself: stop what runs these"
        )?;
    } else {
        writeln!(
            out,
            "only sluice moves {branch}: submit the change to sluice instead"
        )?;
    }
    Ok(())
}

/// 124, S64: this host's copy of `branch` left behind its landing, and
/// why.
fn copy_behind_line(branch: &str, copy: &CopyBehind) -> String {
    let at = copy.at.as_ref().map_or("nothing", |r| r.short());
    format!(
        "this host's {branch} is at {at}, behind the landing {}: {}",
        copy.lacks.short(),
        copy.why
    )
}

/// Session-runs-1 clause 97: every worktree sluice holds ports or runs
/// for, for a person or a tool to release.
fn write_worktrees(places: &[SessionPlace], out: &mut dyn Write) -> Result<()> {
    if places.is_empty() {
        writeln!(out, "no worktree holds ports or runs here")?;
        return Ok(());
    }
    writeln!(out, "worktrees held until released:")?;
    let ww = places
        .iter()
        .map(|p| p.worktree.to_string_lossy().len())
        .max()
        .unwrap_or(0);
    let bw = places.iter().map(|p| p.branch.len()).max().unwrap_or(0);
    for p in places {
        let ports: Vec<String> = p.ports.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let last = p.last_run.as_deref().unwrap_or("never");
        let result = p
            .last_result
            .as_ref()
            .map(|r| format!("  last result {}", last_result_text(r)))
            .unwrap_or_default();
        let mut line = format!(
            "  {:<ww$}  {:<bw$}  last run {last}{result}  {}",
            p.worktree.to_string_lossy(),
            p.branch,
            ports.join(" "),
        );
        if p.gone {
            line = format!(
                "{}  (removed: the next run or round runs its stored release once and frees it)",
                line.trim_end()
            );
        }
        writeln!(out, "{}", line.trim_end())?;
        // 123: held until its release runs again, or its failure is
        // accepted.
        if let Some(f) = &p.release_failed {
            writeln!(
                out,
                "    its release failed ({}): sluice release runs it again, sluice release --accept-failure frees it without",
                match f.tries {
                    1 => "1 try".to_string(),
                    n => format!("{n} tries"),
                }
            )?;
            for l in f.failed.trim_end().lines() {
                writeln!(out, "    | {l}")?;
            }
        }
    }
    writeln!(
        out,
        "release one with: sluice release --worktree <worktree>"
    )?;
    Ok(())
}

/// ADR 0024: each worktree that has checked out a branch sluice lands
/// on, which sluice moves under it.
fn write_held(held: &[Held], out: &mut dyn Write) -> Result<()> {
    for h in held {
        writeln!(
            out,
            "warning: {} has {} checked out: sluice moves {} under it and leaves its files behind; \
             look at it from a worktree switched with git switch --detach",
            h.worktree.display(),
            h.branch,
            h.branch,
        )?;
    }
    Ok(())
}

/// The worktrees `sluice status --json` and `sluice overview --json`
/// list: every one sluice holds, and every one that has checked out a
/// branch sluice lands on, which names it as `holds_queued_branch`.
fn sessions_json(places: &[SessionPlace], held: &[Held]) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = places
        .iter()
        .map(|p| {
            let mut v = worktree_json(p);
            if let Some(h) = held.iter().find(|h| h.worktree == p.worktree) {
                v["holds_queued_branch"] = h.branch.clone().into();
            }
            v
        })
        .collect();
    for h in held {
        if !places.iter().any(|p| p.worktree == h.worktree) {
            out.push(serde_json::json!({
                "worktree": h.worktree,
                "branch": h.branch,
                "holds_queued_branch": h.branch,
            }));
        }
    }
    out
}

/// How a worktree's last run ended, as `sluice status` says it: `passed
/// on 1a2b3c4, recorded`, or a command's `command passed on 1a2b3c4`
/// (session-runs-1 clauses 92, 97).
fn last_result_text(r: &sluice_seams::LastResult) -> String {
    use sluice_seams::{RunOutcome, SessionRunKind};
    let how = match r.outcome {
        RunOutcome::Pass => "passed",
        RunOutcome::Fail => "failed",
        RunOutcome::TimedOut => "timed out",
        RunOutcome::Lost => "lost",
        RunOutcome::Stopped => "stopped",
        RunOutcome::NotRun => "could not run",
    };
    let head = r.head.short();
    match (r.kind, r.recorded) {
        (SessionRunKind::Command, _) => format!("command {how} on {head}"),
        (SessionRunKind::Checks, true) => format!("{how} on {head}, recorded"),
        (SessionRunKind::Checks, false) => format!("{how} on {head}, not recorded"),
    }
}

/// One worktree sluice holds, as `sluice status --json` lists it; its
/// `last_result` only once a run has ended there (97).
fn worktree_json(p: &SessionPlace) -> serde_json::Value {
    let mut v = serde_json::json!({
        "worktree": p.worktree,
        "branch": p.branch,
        "ports": p.ports,
        "last_run": p.last_run,
        "removed": p.gone,
    });
    if let Some(r) = &p.last_result {
        v["last_result"] = serde_json::json!(r);
    }
    if let Some(f) = &p.release_failed {
        v["release_failed"] = serde_json::json!(f);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// core.55: the version is the workspace's in `Cargo.toml`, the tree's
    /// alone, and `--version` says it (record 0111).
    #[test]
    fn the_version_is_the_workspaces() {
        let manifest: toml::Table = toml::from_str(include_str!("../../../Cargo.toml")).unwrap();
        let workspace = manifest["workspace"]["package"]["version"]
            .as_str()
            .expect("Cargo.toml gives [workspace.package] a version");
        assert_eq!(VERSION, workspace);
        let error = Cli::try_parse_from(["sluice", "--version"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(error.to_string(), format!("sluice {workspace}\n"));
    }

    /// 151: `--otel` takes no value, on, or a boolean, `--otel=false` off
    /// for the command, and never the word after it for its value; with
    /// neither it nor `SLUICE_OTEL` it chooses nothing. A command's flags
    /// follow its name, and `--otel` alone is the home view's (design D2).
    #[test]
    fn otel_takes_no_value_or_a_boolean() {
        std::env::remove_var("SLUICE_OTEL");
        for (line, otel) in [
            (&["sluice", "status", "--otel", "main"][..], Some(true)),
            (
                &["sluice", "status", "--otel=false", "main"][..],
                Some(false),
            ),
            (&["sluice", "status", "main"][..], None),
        ] {
            let cli = Cli::try_parse_from(line).unwrap();
            assert_eq!(cli.choices().otel, otel, "{line:?}");
            assert!(
                matches!(&cli.command, Some(Command::Status { target: Some(t), .. }) if t == "main"),
                "{line:?}: {:?}",
                cli.command
            );
        }
        let home = Cli::try_parse_from(["sluice", "--otel"]).unwrap();
        assert_eq!(
            (home.choices().otel, home.command.is_none()),
            (Some(true), true)
        );
    }

    fn names() -> Names {
        let mut names = Names::default();
        names.insert(12, "Batch review cards");
        names.insert(13, "unit/forecast");
        names
    }

    /// 26: a line names a submission with its name the first time it shows
    /// it, and by its number alone after.
    #[test]
    fn a_line_names_each_submission_once() {
        let names = names();
        let mut said = BTreeSet::new();
        assert_eq!(
            hashes(&[12, 13], Some(&names), &mut said),
            "#12 Batch review cards, #13 unit/forecast"
        );
        assert_eq!(hashes(&[12, 14], Some(&names), &mut said), "#12, #14");
        let mut said = BTreeSet::from([12]);
        assert_eq!(
            combination("main", &[12, 13], Some(&names), &mut said),
            "main + #12 + #13 unit/forecast"
        );
        assert_eq!(
            combination("main", &[12, 13], None, &mut BTreeSet::new()),
            "main + #12 + #13"
        );
    }

    /// 26: a name is kept as a line shows it, on one line; one that shows
    /// as nothing leaves its submission to its number.
    #[test]
    fn a_name_is_kept_as_shown() {
        let mut names = Names::default();
        names.insert(12, "Batch review\ncards");
        names.insert(13, "\n");
        assert_eq!(
            hashes(&[12, 13], Some(&names), &mut BTreeSet::new()),
            "#12 Batch review cards, #13"
        );
    }
}
