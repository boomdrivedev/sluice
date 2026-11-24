//! The `sluice` command: its arguments, what it says, and its exits
//! (walkthrough sections 5, 6 and 9).
//!
//! Every command runs against [`Seams`] it is given, so the same code
//! runs over the fakes in tests and over the real profiles that
//! [`profiles`] binds.

pub mod branch;
pub mod caller;
pub mod equip;
pub mod exec;
pub mod hand_over;
pub mod home;
pub mod host_count;
pub mod host_process;
pub mod host_settings;
pub mod identity;
pub mod init;
pub mod instance;
pub mod job;
pub mod overview;
pub mod own_steps;
pub mod pool;
pub mod preview;
pub mod profiles;
pub mod progress;
pub mod protect;
pub mod retry;
pub mod runs;
pub mod scrub;
pub mod sessions;
pub mod signing;
pub mod subscriber;
pub mod telemetry;
pub mod told;
pub mod words;
pub mod workers;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sluice_core::overview::{in_queue, BranchView, CandidateView, Held};
use sluice_core::records::{Beside, BranchHold, CopyBehind, Event, FailedTask, State, Submission};
use sluice_core::{
    Accepted, AmendRefusal, Clock, Collect, CopyDiffers, Forecast, Forecasted, GroupRequest,
    Lookup, Owed, Requester, Seams, Several, Sluice, Status, SubmitHead, SubmitRefusal,
    SubmitRequest, Unread, WithdrawRefusal, Withdrew,
};
use sluice_seams::{Decided, Rev, SessionPlace, Submitter};
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
/// 152, record 0124 (design D10): the forge refused the submission's
/// landing, with the branch where the landing expected it: held, with the
/// forge's words. No reader takes it as a failure (1).
pub const REFUSED: i32 = 6;
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
  6    held: the forge refused its landing; its words are on stdout
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
  $ sluice wait main --pull-request 41   pull request 41 into main, from any folder

Exit codes:
  0    landed, whatever failed after the landing (said on stderr)
  1    failed: it was blamed, or its pull request closed; or no such
       submission on any branch, every branch read; or no such pull
       request into the branch
  2    a conflict
  3    sluice itself failed, or it could not read the forge for 5m and
       whether it landed is not known: the cause is on stderr; or the
       number is on none of the branches it could read, and each branch
       it could not read is named with why
  4    withdrawn
  5    still waiting: --timeout ran out, and the submission stays queued
  6    held: the forge refused its landing; its words are on stdout
  64   a command line sluice cannot read; or the number is on more than
       one branch and no --branch names one: each branch is named; or
       --pull-request with a submission's number
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
    /// 153: the trace context the command was started under, which `main`
    /// reads from the environment it started with and clap never parses;
    /// it names only where the command's span nests (design D1, D7).
    #[arg(skip)]
    pub caller: Option<telemetry::Caller>,
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
        /// A pull request into the branch the target names, by its
        /// number: follows it from any folder of the repository, on any
        /// host.
        #[arg(long, value_name = "N")]
        pull_request: Option<u64>,
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
    /// Follow the host process's passes of the repository, printing each,
    /// until stopped. Nothing needs it: sluice starts its own host
    /// process, which moves every queue on the host (135).
    #[command(hide = true)]
    Run {
        /// Stop after this many passes; a run whose last pass passed a
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
        /// Watch the folders that the lines of standard input name: `add
        /// <folder>` and `remove <folder>`, each document printed with
        /// its `folder`.
        #[arg(long, requires = "watch")]
        stdin: bool,
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
    /// Run as this user's sluice host process, which moves every
    /// repository's queues on the host; started by the commands that need
    /// it (135).
    #[command(hide = true)]
    Host,
    /// Keep this user's sluice host process running, on a machine that
    /// chooses to; nothing needs it.
    ///
    /// sluice starts its own host process when work arrives and lets it end
    /// once nothing is in play, so nothing needs `sluice daemon`. A machine
    /// that runs queues for others, and runs no sluice command itself, may
    /// keep it running under its service manager: it is the host process,
    /// in the foreground, and never ends for being idle (135, 137). It
    /// stops on SIGTERM or SIGINT, and its runs end with it.
    #[command(after_help = "Example:\n  $ sluice daemon")]
    Daemon,
    /// Have this user's sluice host process start afresh as this sluice,
    /// keeping its runs, where it runs another sluice; it starts none.
    ///
    /// A service manager's activation runs it after it installs a new
    /// sluice, in place of a restart, which would end the runs.
    #[command(after_help = "Example:\n  $ sluice hand-over")]
    HandOver,
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
    /// Set sluice up in this working copy.
    #[command(after_help = "Example:\n  $ sluice init actions")]
    Init {
        #[command(subcommand)]
        command: InitCommand,
    },
    /// The pools a branch's runs go to, read back from where they run.
    #[command(after_help = "Example:\n  $ sluice pool check main")]
    Pool {
        #[command(subcommand)]
        command: PoolCommand,
    },
    /// Run by the workflow `sluice init actions` writes: a pool's job's
    /// run, its signature and its record (record 0119).
    #[command(
        hide = true,
        after_help = "Example:\n  $ sluice job run --claim pool/forge/3 --branch main --out \"$RUNNER_TEMP/sluice-run\" --checkout \"$RUNNER_TEMP/sluice-checkout\""
    )]
    Job {
        #[command(subcommand)]
        command: JobCommand,
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

/// 72, 137 (design D25): the host that keeps a branch on its remote, the
/// lead its records name, which takes its pull requests in.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TakenInBy {
    pub host: String,
}

/// 137 (design D25): the pull requests into a branch on its remote that
/// this host follows and does not keep, its queue holding none at its
/// head, each as the host that keeps the branch shows it, for `sluice
/// status <branch>`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Across {
    /// The host the branch's records name as keeping it; `None` where none
    /// does yet.
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
    /// Design D25: the heading of the rows in words: the host that keeps
    /// `branch` and takes its pull requests in, or that none keeps it yet.
    pub fn heading(&self, branch: &str) -> String {
        match &self.taken_in_by {
            Some(by) => format!("pull requests {} takes in:", by.host),
            None => format!("pull requests this host follows; no host keeps {branch} yet:"),
        }
    }

    /// Design D25: one row in words: its number and title (26), then the
    /// submission the check names on the host that posted it and where it
    /// stands there, or that it waits to be taken in by the host that keeps
    /// the branch and on what.
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

/// A map by submission number, written as pairs: a wait's ask carries it
/// inside a request whose fields serde reads flattened, which reads no
/// number from a key (the change `sluice-host-makes-each-change`, design
/// D2).
mod number_keyed {
    use std::collections::BTreeMap;

    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<u64, String>,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&map.iter().collect::<Vec<_>>(), to)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        from: D,
    ) -> Result<BTreeMap<u64, String>, D::Error> {
        let pairs: Vec<(u64, String)> = serde::Deserialize::deserialize(from)?;
        Ok(pairs.into_iter().collect())
    }
}

/// Where a pull request stands, for a submit that follows it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Followed {
    /// Not decided yet: its state, and lines beneath it.
    Open {
        title: String,
        lines: Vec<String>,
        /// 137, design D25: where another host keeps its branch and takes
        /// its pull requests in, that host, as the branch's records name
        /// it; `None` where none keeps it yet, and on the host that keeps
        /// it.
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
        #[serde(with = "number_keyed")]
        names: BTreeMap<u64, String>,
        /// The revision the forge holds as its head; `None` when the forge
        /// returned no pull to read it from.
        head: Option<String>,
        /// Whether it is a draft, which intake does not take in.
        draft: bool,
    },
    /// Its submission on its head landed, failed or conflicts, with what
    /// each submission is called on a pull request.
    Done(
        Submission,
        #[serde(with = "number_keyed")] BTreeMap<u64, String>,
    ),
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

/// 128 (design D26): one cause an intake pass met: what it asked,
/// `<branch>'s pull requests` or `pull/<n>`, and the words it was refused
/// with.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IntakeCause {
    pub asked: String,
    pub said: String,
}

impl IntakeCause {
    /// The cause in a warning's words: `taking in <branch>'s pull
    /// requests: <asked>: <said>`.
    pub fn line(&self, branch: &str) -> String {
        format!(
            "taking in {branch}'s pull requests: {}: {}",
            self.asked, self.said
        )
    }
}

/// What one intake pass did (design D26): whether the branch takes pull
/// requests, whether the pass asked the forge for them, and each cause it
/// met.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intake {
    pub takes: bool,
    pub asked: bool,
    pub causes: Vec<IntakeCause>,
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
    /// each changed, and post each one's state, where this host keeps the
    /// branch on its remote (137; design D21). Whether the branch takes
    /// pull requests, and each cause the pass met, printing none of them
    /// (128; design D26).
    fn intake(&self, seams: Seams<'_>, clock: &dyn Clock, branch: &str) -> Result<Intake>;

    /// After a round of `branch` that stepped: each pull request a submit
    /// opened whose submission that round sent back is marked a draft and
    /// shown held for the next submit, before any follower reads the pass,
    /// with nothing asked of the forge: the requests as this pass's intake
    /// read them, and what the host keeps of them (120, 136; rule core.32;
    /// design D5). Each cause it met, printing none of them (128; design
    /// D26).
    fn settle(&self, _seams: Seams<'_>, _clock: &dyn Clock, _branch: &str) -> Result<Intake> {
        Ok(Intake::default())
    }

    /// 137 (design D25): whether another host keeps `branch` on its remote
    /// and takes its pull requests in, as this process last found it, by
    /// its intake or by following a request into it.
    fn elsewhere(&self, _branch: &str) -> bool {
        false
    }

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

    /// Whether the forge holds pull request `number`, open or closed, into
    /// `branch`: what a wait by pull request reads before it follows one
    /// (record 0130 D3).
    fn holds(&self, seams: Seams<'_>, clock: &dyn Clock, branch: &str, number: u64)
        -> Result<bool>;

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

    fn intake(&self, _: Seams<'_>, _: &dyn Clock, _: &str) -> Result<Intake> {
        Ok(Intake::default())
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

    /// With no forge, no pull request exists.
    fn holds(&self, _: Seams<'_>, _: &dyn Clock, _: &str, _: u64) -> Result<bool> {
        Ok(false)
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
    s: &Sluice<'_>,
    clock: &dyn Clock,
    branch: &str,
    json: bool,
    out: &mut dyn Write,
) -> Intaken {
    thread_local! {
        /// The end of the last pause a round said it passed by.
        static SAID: std::cell::Cell<Option<chrono::DateTime<chrono::Utc>>> =
            const { std::cell::Cell::new(None) };
    }
    let e = match pulls.intake(s.seams(), clock, branch) {
        Ok(i) => return Intaken::noted(s, branch, i.asked, i.causes),
        Err(e) => e,
    };
    // 128 (design D26): a pass that failed whole met one cause, its list.
    let Some(limited) = retry::limited(&e) else {
        let causes = vec![IntakeCause {
            asked: format!("{branch}'s pull requests"),
            said: format!("{e:#}"),
        }];
        return Intaken::noted(s, branch, true, causes);
    };
    let waited = e.downcast_ref::<retry::Spent>().is_some();
    if !waited && SAID.get() != Some(limited.until) {
        SAID.set(Some(limited.until));
        let line = format!("{branch}'s pull requests passed by: {limited}");
        if json || writeln!(out, "{line}").is_err() {
            telemetry::warn(&format!("sluice: {line}"));
        }
    }
    Intaken {
        causes: Vec::new(),
        logged: Vec::new(),
        limited: Some(e),
    }
}

/// After a round of `branch` that stepped, what it sent back of a pull
/// request shown on it, a submit's request marked a draft, with nothing
/// asked of the forge (120, 136; design D5): each cause it met noted as an
/// intake's are, ending none. A spent allowance is said by the next
/// intake, which meets it first.
fn settle(pulls: &dyn PullRequests, s: &Sluice<'_>, clock: &dyn Clock, branch: &str) -> Intaken {
    match pulls.settle(s.seams(), clock, branch) {
        Ok(i) => Intaken::noted(s, branch, false, i.causes),
        Err(e) if retry::limited(&e).is_some() => Intaken {
            causes: Vec::new(),
            logged: Vec::new(),
            limited: Some(e),
        },
        Err(e) => {
            let causes = vec![IntakeCause {
                asked: format!("{branch}'s pull requests"),
                said: format!("{e:#}"),
            }];
            Intaken::noted(s, branch, false, causes)
        }
    }
}

/// What [`intake`] met (design D26, D27): each cause, which a step and a
/// submit say; those whose lines its note wrote to the branch's log, which
/// a pass says in the host process's log; and a spent allowance, said once
/// for its pause.
struct Intaken {
    causes: Vec<IntakeCause>,
    logged: Vec<IntakeCause>,
    limited: Option<anyhow::Error>,
}

impl Intaken {
    /// 133, rule core.28 (design D27): `causes`, an intake pass's of
    /// `branch`, noted in this host's refusal of its intake, each logged
    /// once; a pass that `asked` the forge and met none ends the refusal,
    /// and one that asked nothing leaves it as it stands. A note that fails
    /// is warned of, and logs nothing.
    fn noted(s: &Sluice<'_>, branch: &str, asked: bool, causes: Vec<IntakeCause>) -> Intaken {
        let remote = s.remote_of(branch).ok().flatten().unwrap_or_default();
        let noted = match (causes.is_empty(), asked) {
            (true, false) => Ok(Vec::new()),
            (true, true) => s.note_intake_read(branch).map(|_| Vec::new()),
            (false, _) => {
                let read: Vec<sluice_seams::ReadRefused> = causes
                    .iter()
                    .map(|c| sluice_seams::ReadRefused {
                        remote: remote.clone(),
                        asked: c.asked.clone(),
                        said: c.said.clone(),
                        note: None,
                    })
                    .collect();
                s.note_intake_refused(branch, &read)
            }
        };
        let logged = match noted {
            Ok(logged) => logged
                .into_iter()
                .map(|r| IntakeCause {
                    asked: r.asked,
                    said: r.said,
                })
                .collect(),
            Err(e) => {
                telemetry::warn(&format!(
                    "sluice: warning: keeping {branch}'s intake refusal: {e:#}"
                ));
                Vec::new()
            }
        };
        Intaken {
            causes,
            logged,
            limited: None,
        }
    }

    /// Each cause, said on stderr as a warning: a submit's own first
    /// intake's (design D26).
    fn say(&self, branch: &str) {
        for c in &self.causes {
            telemetry::warn(&format!("sluice: warning: {}", c.line(branch)));
        }
    }
}

/// A branch command, as `sluice branch` and its request to `sluice host`
/// name it (the change `sluice-host-makes-each-change`, design D1).
#[derive(Subcommand, Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
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
pub enum PoolCommand {
    /// Read each CodeBuild pool the policy at the branch's revision names
    /// back from AWS: who the host reaches it as, each run project on a
    /// reserved-capacity fleet of its system with the buildspec this
    /// release writes and a timeout longer than the branch's, the sign
    /// project's buildspec, and the key against the policy's half. Exits 1
    /// naming each that fails; says where the branch does not test side by
    /// side at the fleets' size.
    #[command(
        after_help = "Examples:\n  $ sluice pool check main\n  $ sluice pool check main --profile ops"
    )]
    Check {
        /// The branch whose policy names the pools.
        branch: String,
        /// The AWS profile to read the pools as; else the host's
        /// [aws] profile, else the AWS chain's.
        #[arg(long)]
        profile: Option<String>,
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

#[derive(Subcommand, Debug)]
pub enum InitCommand {
    /// Write the workflows a pool on GitHub Actions runs, append its signer
    /// and pool to the policy where it names no pool on GitHub, and say what
    /// to set on GitHub. A workflow that differs is left, exit 1, unless
    /// --force.
    #[command(after_help = "Examples:\n  $ sluice init actions\n  $ sluice init actions --force")]
    Actions {
        /// The working copy to set up.
        #[arg(long, default_value = ".")]
        worktree: PathBuf,
        /// Write a workflow that differs from what sluice writes.
        #[arg(long)]
        force: bool,
    },
    /// Write the CloudFormation template of a pool on a CodeBuild fleet,
    /// its fleets, projects, key, bucket, rule and roles, and say how to
    /// deploy it; a template that differs is left, exit 1, unless --force.
    /// With --stack, read the deployed stack's key and append its signer
    /// and pool to the policy where it names neither.
    #[command(
        after_help = "Examples:\n  $ sluice init codebuild\n  $ sluice init codebuild --systems linux-x64 linux-arm64\n  $ sluice init codebuild --stack sluice --region us-east-1"
    )]
    Codebuild {
        /// The working copy to set up.
        #[arg(long, default_value = ".")]
        worktree: PathBuf,
        /// The systems to run, by the policy's names; else each its
        /// [systems] names that CodeBuild runs on Linux.
        #[arg(long, num_args = 1..)]
        systems: Vec<String>,
        /// Write a template that differs from what sluice writes.
        #[arg(long, conflicts_with = "stack")]
        force: bool,
        /// The stack the template was deployed as: its key read and the
        /// pool named in the policy.
        #[arg(long)]
        stack: Option<String>,
        /// With --stack, the stack's region; else the AWS chain's.
        #[arg(long, requires = "stack")]
        region: Option<String>,
        /// With --stack, the AWS profile to read the stack's key as; else
        /// the AWS chain's.
        #[arg(long, requires = "stack")]
        profile: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum JobCommand {
    /// Start the run a claim holds, or a pull request's head, and keep its
    /// report and what proves its keys in --out; records nothing, writes
    /// no state and holds no key. Keeps the checkout it made, with its
    /// ports and scratch, whatever its run came to, for the job's own steps
    /// after it, and names it in --checkout's file: the checkout's path on
    /// its first line, its place's name on its second; `sluice job release`
    /// releases it after those steps. Where it made no checkout it writes
    /// no file; stopped, by Ctrl-C or SIGTERM, it releases the checkout at
    /// once and writes none. Exits 0 once the report is kept, and 3 with
    /// why where the run reported nothing.
    #[command(
        after_help = "Examples:\n  $ sluice job run --claim pool/forge/3 --branch main --out run --checkout checkout\n  $ sluice job run --head 4f2c9e1 --branch main --system linux-x64 --out run --checkout checkout"
    )]
    Run {
        /// The claim the pool's job runs under, `pool/<pool>/<n>`.
        #[arg(long, conflicts_with = "head")]
        claim: Option<String>,
        /// A pull request's head, run with no claim.
        #[arg(long, required_unless_present = "claim")]
        head: Option<String>,
        /// The branch the run's combination, or the head, lands on.
        #[arg(long)]
        branch: String,
        /// With --head, the system the policy names the runner as.
        #[arg(long, requires = "head")]
        system: Option<String>,
        /// The directory the run's report and proof are kept in.
        #[arg(long)]
        out: PathBuf,
        /// The file that names the checkout kept for the job's own steps,
        /// which `sluice job release --checkout` reads.
        #[arg(long)]
        checkout: PathBuf,
    },
    /// Release the checkout `sluice job run` kept, named in --checkout's
    /// file, after the job's own steps, whatever they came to: by the
    /// release the policy names, its ports and scratch freed, and the file
    /// removed. Run in the clone `sluice job run` ran in. A release that
    /// fails is said, and appended to --out's `why` where it is given,
    /// exiting 1, deciding nothing of the run's outcome. Where the file
    /// does not exist it releases nothing, says so and exits 0.
    #[command(
        after_help = "Example:\n  $ sluice job release --checkout \"$RUNNER_TEMP/sluice-checkout\" --out \"$RUNNER_TEMP/sluice-run\""
    )]
    Release {
        /// The file `sluice job run --checkout` named its checkout in.
        #[arg(long)]
        checkout: PathBuf,
        /// The run job's directory, whose `why` a release that fails is
        /// appended to.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Keep of a run job's report only the keys the combination's tree
    /// bears out, running nothing of it, and write the subjects and the
    /// predicate `actions/attest` signs to --out; where none is borne out,
    /// or the run job kept no report, it writes no subjects. With --build,
    /// a CodeBuild pool's run build is read from CodeBuild, its hand-over
    /// kept in --out's `run` only as CodeBuild recorded it, and what the
    /// tree bears out signed with the pool's key in KMS into --out's
    /// `record.json`.
    #[command(
        after_help = "Examples:\n  $ sluice job sign --claim pool/forge/3 --branch main --from run --out sign\n  $ sluice job sign --build \"$SLUICE_RUN_BUILD\" --out sign"
    )]
    Sign {
        /// The claim the pool's job ran under.
        #[arg(long, conflicts_with_all = ["head", "build"])]
        claim: Option<String>,
        /// A pull request's head, signed with no claim.
        #[arg(long, required_unless_present_any = ["claim", "build"], conflicts_with = "build")]
        head: Option<String>,
        /// A CodeBuild pool's run build, by its ARN: its claim and branch
        /// are read from it.
        #[arg(long)]
        build: Option<String>,
        /// The branch the run's combination, or the head, lands on.
        #[arg(long, required_unless_present = "build", conflicts_with = "build")]
        branch: Option<String>,
        /// With --head, the system the head's run job ran as.
        #[arg(long, requires = "head")]
        system: Option<String>,
        /// The run job's directory, as `sluice job run --out` kept it.
        #[arg(long, required_unless_present = "build", conflicts_with = "build")]
        from: Option<PathBuf>,
        /// The directory the subjects and the predicate are written to.
        #[arg(long)]
        out: PathBuf,
    },
    /// Add the sign job's bundle to the signer's runs and write the run
    /// ended and closed under its claim in one write; with no bundle, the
    /// run ended with no outcome. Refused, exit 1 with nothing written,
    /// where the claim no longer holds the run. With --head, a pull
    /// request's head run's bundle is added, under no claim, writing no
    /// state.
    /// With --build, a CodeBuild pool's run build's claim and branch are
    /// read from CodeBuild again, and --from is the sign build's directory,
    /// whose `record.json`, where it holds one, is added.
    #[command(
        after_help = "Examples:\n  $ sluice job record --claim pool/forge/3 --branch main --bundle attestation.json\n  $ sluice job record --head 4f2c9e1 --branch main --bundle attestation.json\n  $ sluice job record --build \"$SLUICE_RUN_BUILD\" --from sign"
    )]
    Record {
        /// The claim the pool's job ran under.
        #[arg(long, conflicts_with_all = ["head", "build"])]
        claim: Option<String>,
        /// A pull request's head, whose signed results are added with no
        /// claim.
        #[arg(
            long,
            required_unless_present_any = ["claim", "build"],
            requires = "bundle",
            conflicts_with = "build"
        )]
        head: Option<String>,
        /// A CodeBuild pool's run build, by its ARN: its claim and branch
        /// are read from it, and --from is the sign build's directory.
        #[arg(long, requires = "from", conflicts_with = "bundle")]
        build: Option<String>,
        /// The branch the run's combination lands on.
        #[arg(long, required_unless_present = "build", conflicts_with = "build")]
        branch: Option<String>,
        /// The bundle `actions/attest` made of the run's statement.
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// The run job's directory: each check of its report the sign job
        /// left out ends with no result, and where it holds no report it
        /// says why.
        #[arg(long)]
        from: Option<PathBuf>,
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

/// Whether Ctrl-C was pressed since [`stop_on_interrupt`] took it.
pub(crate) fn was_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Ctrl-C ends a wait and nothing else: the submission stays queued (5).
/// A session's command, which `sluice host` runs, asks it to stop as
/// Ctrl-C stopped it (the change `sluice-host-runs-a-sessions-checks`,
/// design D4).
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

/// Record 0016 (design D8 of the change
/// `a-codebuild-fleet-runs-a-queues-checks`): a pool stopping a job's run
/// ends it as Ctrl-C does, so `sluice job run` ends its run and releases
/// its checkout before it exits.
pub fn stop_on_terminate() -> Result<()> {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
    let action = SigAction::new(
        SigHandler::Handler(interrupted),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: the handler only stores to an atomic.
    unsafe { sigaction(Signal::SIGTERM, &action) }?;
    Ok(())
}

/// A pause between rounds that skips its time and moves its own clock on
/// by it, a clock nothing else moves: a test follows a wait over minutes
/// in a moment, with every time it reads, a deadline's and the host's
/// interval's among them, as the wait would have read it.
pub struct Skipped {
    every: std::time::Duration,
    start: std::cell::Cell<chrono::DateTime<chrono::Utc>>,
    moved: std::cell::Cell<std::time::Duration>,
}

impl Skipped {
    pub fn new(every: std::time::Duration) -> Skipped {
        Skipped {
            every,
            start: std::cell::Cell::new(chrono::Utc::now()),
            moved: std::cell::Cell::default(),
        }
    }
}

impl Skipped {
    /// Its time set to `at`, a command's that asked for a pass (design
    /// D14): each pass of a test's host process is at the time of the
    /// command that asked for it, however many ask, and a command started
    /// later starts from its own time.
    pub fn set(&self, at: chrono::DateTime<chrono::Utc>) {
        match (at - self.start.get()).to_std() {
            Ok(moved) => self.moved.set(moved),
            // A command's time before the clock's start: the clock starts
            // there, so the pass is at the command's own time.
            Err(_) => {
                self.start.set(at);
                self.moved.set(std::time::Duration::ZERO);
            }
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
        self.start.get() + chrono::TimeDelta::from_std(self.moved.get()).unwrap_or_default()
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

    /// The command's words as `sluice --help` lists them: `sluice exec`,
    /// `sluice job run`, and `sluice` for the home view, which name its
    /// span in its caller's trace (153; design D2).
    pub fn words(&self) -> String {
        let words = match &self.command {
            None => return "sluice".into(),
            Some(command) => match command {
                Command::Submit { .. } => "submit",
                Command::Wait { .. } => "wait",
                Command::Inbox { .. } => "inbox",
                Command::Confirm { .. } => "confirm",
                Command::Step { .. } => "step",
                Command::Run { .. } => "run",
                Command::Status { .. } => "status",
                Command::Overview { .. } => "overview",
                Command::Amend { .. } => "amend",
                Command::Withdraw { .. } => "withdraw",
                Command::Exec { .. } => "exec",
                Command::Equip { .. } => "equip",
                Command::Preview { .. } => "preview",
                Command::Previews { .. } => "previews",
                Command::Release { .. } => "release",
                Command::Supervise { .. } => "supervise",
                Command::Host => "host",
                Command::Daemon => "daemon",
                Command::HandOver => "hand-over",
                Command::Protect { .. } => "protect",
                Command::Hook { .. } => "hook",
                Command::Branch { command } => match command {
                    BranchCommand::Create { .. } => "branch create",
                    BranchCommand::Delete { .. } => "branch delete",
                    BranchCommand::Accept { .. } => "branch accept",
                },
                Command::Forge { command } => match command {
                    ForgeCommand::Check { .. } => "forge check",
                },
                Command::Init { command } => match command {
                    InitCommand::Actions { .. } => "init actions",
                    InitCommand::Codebuild { .. } => "init codebuild",
                },
                Command::Pool { command } => match command {
                    PoolCommand::Check { .. } => "pool check",
                },
                Command::Job { command } => match command {
                    JobCommand::Run { .. } => "job run",
                    JobCommand::Sign { .. } => "job sign",
                    JobCommand::Record { .. } => "job record",
                    JobCommand::Release { .. } => "job release",
                },
                Command::Credential { .. } => "credential",
            },
        };
        format!("sluice {words}")
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

/// What a kept `sluice run`, a follower of sluice's host process, carries
/// from one pass to the next (135; design D11): the passes it followed
/// toward its `--rounds`, and whether it said that nothing needs it. It
/// holds nothing its settings decide but its own telemetry.
pub struct Following {
    rounds: Option<usize>,
    /// How long it waits for the host process's next pass.
    tick: Duration,
    n: usize,
    told: bool,
}

impl Following {
    /// A `sluice run` that has followed no pass yet, ending after
    /// `rounds` passes where it names a bound.
    pub fn new(rounds: Option<usize>) -> Following {
        Following {
            rounds,
            tick: tick(Some(&Command::Run { rounds })),
            n: 0,
            told: false,
        }
    }

    /// A `sluice run` started afresh from one that handed it `doc`, ending
    /// once `rounds` passes, those already followed among them, where it
    /// names a bound; it says again nothing the run said (135; record
    /// 0107; design D11).
    pub fn handed(rounds: Option<usize>, doc: &hand_over::HandOver) -> Following {
        Following {
            n: doc.passes,
            told: doc.told,
            ..Following::new(rounds)
        }
    }

    /// How long it waits for the host process's next pass.
    pub fn tick(&self) -> Duration {
        self.tick
    }

    /// The passes followed toward `--rounds`.
    pub fn passes(&self) -> usize {
        self.n
    }

    /// Whether it said that nothing needs it.
    pub fn told(&self) -> bool {
        self.told
    }

    /// Its start, where it was not handed over: said once that nothing
    /// needs `sluice run` (design D11). `sluice host` checks the host's
    /// systems as it binds the repository (87).
    pub fn start(&mut self) -> Result<()> {
        if !self.told {
            // D13 of `sluice-runs-as-a-daemon`: a machine that chooses to
            // keep the host process running keeps `sluice daemon`.
            telemetry::warn(
                "sluice: sluice starts its own host process; nothing needs `sluice run`, and \
                 `sluice daemon` keeps it running on a machine that chooses to",
            );
            self.told = true;
        }
        Ok(())
    }

    /// One pass followed: the host process's next pass's report, asked of
    /// it where passes are taken only when asked, its lines printed as a
    /// pass of `sluice run` printed them (135; design D11). A follower
    /// that could not reach the process, or was told no pass within its
    /// tick, pauses and reaches it again at its next.
    pub fn next(
        &mut self,
        runner: &mut subscriber::Runner<'_>,
        clock: &dyn Clock,
        pause: &dyn Pause,
        out: &mut dyn Write,
    ) -> Result<Driven> {
        let Some(report) = runner.next(clock) else {
            if INTERRUPTED.load(Ordering::SeqCst) || !pause.pause() {
                return Ok(Driven::Ended(STOPPED));
            }
            return Ok(Driven::Next);
        };
        for line in &report.lines {
            writeln!(out, "{line}")?;
        }
        out.flush()?;
        self.n += 1;
        if self.rounds.is_some_and(|r| self.n >= r) {
            // 26: one line per branch, as `main` writes a failed step's,
            // said whether or not stdout said it this pass.
            for (branch, words) in &report.failed {
                telemetry::error(&format!("sluice: {branch}: {words}"));
            }
            return Ok(Driven::Ended(if report.failed.is_empty() {
                0
            } else {
                INTERNAL
            }));
        }
        // Where passes are taken only when asked, the next is asked after
        // the follower's own pause.
        if runner.asked_only() && !pause.pause() {
            return Ok(Driven::Ended(STOPPED));
        }
        Ok(Driven::Next)
    }
}

/// What a pass of `sluice run` ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driven {
    /// The next pass is due.
    Next,
    /// The run ends with this exit: its `--rounds` reached, or its pause
    /// stopped.
    Ended(i32),
}

/// Run one command; returns its exit code. Started under a caller's trace
/// context, its whole run is one span in the caller's trace (153; design
/// D2).
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
    let words = cli.words();
    let caller = cli.caller.clone();
    telemetry::command(caller.as_ref(), &words, || {
        dispatch(cli, seams, host, pushers, pulls, clock, pause, out)
    })
}

/// A round of a branch as `sluice host` answered `step` with it, printed
/// as `sluice step` prints it (rule core.14; design D10): every warning
/// its round's intake met on stderr (128; design D26), what it said, and
/// its line, or its document under `--json`.
pub fn say_step(stepped: &serde_json::Value, json: bool, out: &mut dyn Write) -> Result<i32> {
    let warnings = stepped["warnings"].as_array().cloned().unwrap_or_default();
    for w in warnings.iter().filter_map(|w| w.as_str()) {
        telemetry::warn(&format!("sluice: warning: {w}"));
    }
    let said = stepped["said"].as_array().cloned().unwrap_or_default();
    for line in said.iter().filter_map(|l| l.as_str()) {
        if json {
            telemetry::say(line);
        } else {
            writeln!(out, "{line}")?;
        }
    }
    if json {
        writeln!(out, "{}", stepped["document"])?;
    } else {
        writeln!(out, "{}", stepped["line"].as_str().unwrap_or_default())?;
    }
    Ok(0)
}

/// `sluice submit`, made in `sluice host` (the change
/// `sluice-host-makes-each-change`, design D2): what it made, which a
/// waiting submit's command follows. Started under a caller's trace
/// context, it is one span in the caller's trace (153; design D2).
pub fn submit_run(
    cli: Cli,
    seams: Seams<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    out: &mut dyn Write,
) -> Result<Submitted> {
    let words = cli.words();
    let caller = cli.caller.clone();
    let Some(Command::Submit {
        branch,
        after,
        priority,
        no_wait,
        subject,
        head,
        status,
        worktree,
        json,
        confirm,
        confirm_landing,
        ..
    }) = cli.command
    else {
        anyhow::bail!("the request is no submit");
    };
    let ask = host_process::protocol::SubmitAsk {
        branch,
        after,
        priority,
        no_wait,
        subject,
        head,
        status,
        worktree,
        json,
        confirm,
        confirm_landing,
    };
    let mut made = None;
    telemetry::command(caller.as_ref(), &words, || {
        let s = Sluice::new(seams, clock).as_peer(host.peer());
        let here = host_process::Here {
            s: &s,
            host,
            pulls,
            clock,
        };
        // 153 (design D4): the context the command was started under, which
        // each submission it makes keeps.
        let trace = caller.as_ref().map(|c| c.traceparent.clone());
        let submitted = submit_command(&here, pushers, ask, false, trace, out)?;
        let code = match &submitted {
            Submitted::Done(code) => *code,
            Submitted::Wait { .. } => 0,
        };
        made = Some(submitted);
        Ok(code)
    })?;
    made.ok_or_else(|| anyhow::anyhow!("the submit made nothing"))
}

/// The command `cli` names, run.
fn dispatch(
    cli: Cli,
    seams: Seams<'_>,
    host: &dyn exec::Host,
    pushers: &dyn Pushers,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    // Record 0119 (design D11): a pool's job holds the records only as it
    // read them, and writes none.
    let cli = match cli.command {
        Some(Command::Job { command }) => {
            return job::run(command, seams, host, clock, pause, out);
        }
        command => Cli { command, ..cli },
    };
    let s = Sluice::new(seams, clock).as_peer(host.peer());
    // 129 (design D16): a one-shot read, a status in each form, a one-shot
    // overview, the home view and a forecast, asks sluice's host process
    // for a reading of the repository current within its tick, starting it
    // where none answers, and then reads every branch as the host holds it,
    // fetching nothing of its own: a forecast beside a reader that keeps
    // its tick is answered at once, and forms from that reader's document
    // where it holds what the forecast forms from.
    let one_shot = matches!(
        &cli.command,
        None | Some(Command::Status { .. })
            | Some(Command::Overview { watch: false, .. })
            | Some(Command::Submit { check: true, .. })
    );
    let (s, _reading) = match one_shot {
        true => {
            let reading = host_process::ask::read(
                &host_process::Here {
                    s: &s,
                    host,
                    pulls,
                    clock,
                },
                false,
            );
            (s.holding(), reading)
        }
        false => (s, None),
    };
    let here = host_process::Here {
        s: &s,
        host,
        pulls,
        clock,
    };
    let Some(command) = cli.command else {
        return home::run(&s, host, &cli.worktree, cli.json, out);
    };
    // 153 (design D4): the context the command was started under, which
    // each submission it makes keeps, whether or not it sends telemetry.
    let trace = cli.caller.as_ref().map(|c| c.traceparent.clone());
    // 135 (design D4): the repository, where it has sluice, listed for the
    // host process to find again.
    if matches!(
        command,
        Command::Exec { .. } | Command::Preview { .. } | Command::Equip { .. }
    ) {
        host_process::ask::note(&here);
    }
    // 135 (design D15): what changes a queue tells the host process,
    // starting it where none answers.
    let serve = || host_process::ask::serve(&here);
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
            let ask = host_process::protocol::SubmitAsk {
                branch,
                after,
                priority,
                no_wait,
                subject,
                head,
                status,
                worktree,
                json,
                confirm,
                confirm_landing,
            };
            match submit_command(&here, pushers, ask, check, trace, out)? {
                Submitted::Done(code) => Ok(code),
                // The change `sluice-host-makes-each-change`, design D2:
                // the wait follows `sluice host`, which takes its steps.
                Submitted::Wait { ask, queued } => follow_wait(
                    host.host_process(),
                    Some(&here),
                    caller::folder()?,
                    ask,
                    queued.then_some(State::Queued),
                    None,
                    clock,
                    pause,
                    out,
                ),
            }
        }
        Command::Wait {
            target,
            timeout,
            worktree,
            json,
            confirm,
            confirm_landing,
            branch,
            pull_request,
        } => {
            let deadline = timeout.map(|t| {
                clock.now() + chrono::Duration::from_std(t).unwrap_or(chrono::Duration::MAX)
            });
            // The change `sluice-host-answers-every-read`, design D8, and
            // `sluice-host-makes-each-change`, design D2: the wait follows
            // `sluice host`, which takes each of its steps, a pull
            // request's among them.
            let ask = host_process::protocol::WaitAsk {
                target,
                branch,
                pull_request,
                worktree: Some(worktree),
                json,
                confirm,
                confirm_landing,
                ..Default::default()
            };
            follow_wait(
                host.host_process(),
                Some(&here),
                caller::folder()?,
                ask,
                None,
                deadline,
                clock,
                pause,
                out,
            )
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
        Command::Job { .. } => unreachable!("a job is run before the instance"),
        Command::Init {
            command: InitCommand::Actions { worktree, force },
        } => init::actions(&worktree, force, out),
        Command::Init {
            command:
                InitCommand::Codebuild {
                    worktree,
                    systems,
                    force,
                    stack: None,
                    ..
                },
        } => init::codebuild(&worktree, &systems, force, out),
        Command::Init {
            command:
                InitCommand::Codebuild {
                    worktree,
                    systems,
                    stack: Some(stack),
                    region,
                    profile,
                    ..
                },
        } => init::codebuild_stack(
            &worktree,
            &systems,
            &stack,
            region.as_deref(),
            profile.as_deref(),
            out,
        ),
        Command::Step { branch, json } => {
            // Rule core.14 (design D10): the host process's round of the
            // branch, between its passes, printed as a step prints it.
            let stepped = host.host_process().step(&here, &branch)?;
            say_step(&stepped, json, out)
        }
        Command::Run { rounds } => {
            // 135 (design D11): kept so a host's unit that runs it keeps
            // working, as a follower of the host process, which moves every
            // repository on the host; it runs no round itself.
            let mut following = Following::new(rounds);
            following.start()?;
            let folder = std::env::current_dir()?;
            let tick = following.tick();
            let mut runner =
                subscriber::Runner::new(host.host_process(), Some(&here), folder, tick);
            loop {
                if let Driven::Ended(code) = following.next(&mut runner, clock, pause, out)? {
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
            // 71, 150 (design D12): the host's room, as the overview has it.
            let (room, process) = overview::room_and_process(&s, host, clock)?;
            if json {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "worktrees": sessions_json(&places, &held),
                        "room": room,
                        "host_process": process,
                    }))?
                )?;
            } else {
                write_worktrees(&places, out)?;
                write_held(&held, out)?;
                if let Some(line) = overview::room_line(&room) {
                    writeln!(out, "{line}")?;
                }
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
                    let mut doc = with_waiting(&s, &sub, decided_document(&s, &sub, None)?);
                    // 51, D13: a decided submission's delivery.
                    if let Some(d) = s.delivery_of(&sub)? {
                        doc["delivery"] = d.to_json();
                    }
                    // Record 0122 (design D4): left for its host in a closed
                    // remote home.
                    if let Some((home, host)) = s.left_for_host(&sub.branch, sub.id)? {
                        doc["left_for"] = serde_json::json!({ "home": home, "host": host });
                    }
                    with_checks(&s, &sub, &mut doc);
                    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                    Ok(0)
                }
                Lookup::Found(sub) => {
                    // Record 0122 (design D5, D6): carried under another
                    // number, named as it is now.
                    if sub.id != id {
                        let from = sub
                            .carried_from
                            .as_deref()
                            .unwrap_or(sluice_core::records::HERE);
                        writeln!(
                            out,
                            "submission {id} is #{} now: {}'s queue moved, and carried it from {} under the next number",
                            sub.id,
                            sub.branch,
                            sluice_core::home::describe(from)
                        )?;
                    }
                    let left = s.left_for_host(&sub.branch, sub.id)?;
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
                    // Record 0122 (design D4): a submission its host has not
                    // taken home says so.
                    if let Some((home, host)) = left {
                        writeln!(
                            out,
                            "#{id} is in {home}'s closed home, waiting for {} to take it home",
                            host.as_deref().unwrap_or("the host that made it")
                        )?;
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
                // 129 (design D16): as the host last kept them; the host
                // process's reading reads each one that falls due.
                let across = pulls
                    .kept_across(s.seams(), clock, &target)
                    .unwrap_or_else(|e| {
                        telemetry::warn(&format!("sluice: warning: {e:#}"));
                        None
                    });
                // 51, D13: what the branch's readers are owed, shown after
                // its queue.
                let owed = s.owed_on_branch(&target)?;
                if json {
                    let mut doc = status_json(&s, &status, &shown)?;
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
        // The change `sluice-host-answers-every-read`, design D7: a watch
        // follows `sluice host` on a subscription of its folder.
        Command::Overview { watch: true, .. } => {
            let folder = std::env::current_dir()?;
            subscriber::watch(host.host_process(), Some(&here), folder, clock, pause, out)
        }
        Command::Overview { json, .. } => overview::run(&s, host, clock, pulls, json, out),
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
                serve();
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
                serve();
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
                serve();
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
        } if name.starts_with(CHECKOUT) => {
            let code = release_checkout(&s, &name, accept_failure, json, out);
            serve();
            code
        }
        Command::Release {
            name: Some(name),
            accept_failure,
            json,
            ..
        } => {
            let code = preview::release(&s, host, &name, accept_failure, json, out);
            serve();
            code
        }
        Command::Release {
            name: None,
            worktree,
            accept_failure,
            json,
        } => {
            let released = exec::release(&s, host, &worktree, accept_failure, json, out)?;
            // 135, record 0120 as amended 2026-11-17 (D10): only a release
            // that freed something can move a queue, so one that freed
            // nothing, or failed, tells no host process and starts none,
            // and a release with nothing to free makes nothing (core.10).
            if released.released {
                serve();
            }
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
        Command::Host => host_process::run(telemetry::Choices::default(), Default::default()),
        Command::Daemon => host_process::daemon(telemetry::Choices::default(), Default::default()),
        // main answers it before any repository is read.
        Command::HandOver => Ok(0),
        Command::Protect { remove, json } => {
            let dir = caller::folder()?;
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
        Command::Branch { command } => {
            // D15: a branch made, accepted or removed changes what the
            // host process reads of the repository.
            let changes = matches!(
                command,
                BranchCommand::Create { .. }
                    | BranchCommand::Accept { .. }
                    | BranchCommand::Delete { .. }
            );
            let code = branch::run(&s, command, out)?;
            if changes && code == 0 {
                serve();
            }
            Ok(code)
        }
        Command::Pool {
            command: PoolCommand::Check { branch, profile },
        } => {
            let aws = match profile {
                Some(p) => instance::Aws { profile: Some(p) },
                None => instance::Instance::read()?.aws,
            };
            pool::check(&s, &branch, &aws, out)
        }
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
        // The hook binds no repository: it asks `sluice host` (the change
        // `the-git-hook-asks-sluice-host`, design D1).
        Command::Hook { name, .. } => anyhow::bail!("sluice's {name} hook binds no repository"),
    }
}

/// `sluice submit`, as `sluice host` makes it (the change
/// `sluice-host-makes-each-change`, design D2): the submission written,
/// its pull request opened where its branch takes them, and what it says;
/// then, for a waiting submit, the wait it follows, which its command
/// follows. With `check`, the forecast, which writes nothing.
pub fn submit_command(
    here: &host_process::Here<'_, '_>,
    pushers: &dyn Pushers,
    ask: host_process::protocol::SubmitAsk,
    check: bool,
    trace: Option<String>,
    out: &mut dyn Write,
) -> Result<Submitted> {
    let (s, host, pulls, clock) = (here.s, here.host, here.pulls, here.clock);
    let host_process::protocol::SubmitAsk {
        branch,
        after,
        priority,
        no_wait,
        subject,
        head,
        status,
        worktree,
        json,
        confirm,
        confirm_landing,
    } = ask;
    let confirms = Confirms::of(confirm, confirm_landing);
    // 124, D1: a submit by name reads no worktree, and has no last
    // submission of one to report.
    if head.is_some() && !check {
        let why = if !worktree.is_empty() {
            Some(
                "--head names what is submitted, from anywhere in the repository; give no \
                 --worktree with it",
            )
        } else if status {
            Some(
                "--status reads a worktree's last submission; a branch's are in sluice status \
                 <target> --all",
            )
        } else if subject.len() > 1 {
            Some("--head submits one branch; give one --subject with it, or none")
        } else {
            None
        };
        if let Some(why) = why {
            writeln!(out, "sluice submit: {why}")?;
            return Ok(Submitted::Done(USAGE));
        }
    }
    let worktree = if worktree.is_empty() {
        vec![".".to_string()]
    } else {
        worktree
    };
    // 125: several worktrees are one act, which neither forecasts nor
    // reports one worktree's last submission.
    if worktree.len() > 1 && (check || status) {
        let flag = if check { "--check" } else { "--status" };
        writeln!(
            out,
            "sluice submit: {flag} takes one worktree; give --worktree once with it"
        )?;
        return Ok(Submitted::Done(USAGE));
    }
    // C.5 pull request, 23: a subject is one line; 125: a group gives one
    // per worktree, in their order, or none.
    let why = if subject.iter().any(|s| s.trim().is_empty()) {
        Some("--subject is empty; a subject is one line saying what lands".to_string())
    } else if subject.iter().any(|s| s.contains(['\n', '\r'])) {
        Some("a subject is one line; --subject holds a line break".to_string())
    } else if !subject.is_empty() && subject.len() != worktree.len() {
        let given = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
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
        return Ok(Submitted::Done(USAGE));
    }
    // 137 (design D22): a branch that takes pull requests on records each
    // host keeps is refused, before anything is pushed or opened.
    if let Err(e) = takes_pulls(s, &branch) {
        if let Some(refused) = no_intake(&e) {
            let text = format!("refused: {refused}");
            told::tell(out, json, Kind::Refused, &text, &[])?;
            return Ok(Submitted::Done(CONFLICT));
        }
    }
    if worktree.len() > 1 {
        let req = GroupRequest {
            branch,
            heads: worktree.into_iter().map(SubmitHead::Worktree).collect(),
            after,
            priority,
            subjects: subject,
            trace,
        };
        if takes_pulls(s, &req.branch)? {
            return submit_pull_group(s, host, pulls, clock, req, no_wait, json, out);
        }
        return submit_group(s, host, pushers, pulls, clock, req, no_wait, json, out);
    }
    let worktree = worktree.into_iter().next().unwrap_or_else(|| ".".into());
    let subject = subject.into_iter().next();
    if check {
        let code = forecast(
            here,
            pushers,
            &worktree,
            head.as_deref(),
            &branch,
            &after,
            json,
            out,
        )?;
        return Ok(Submitted::Done(code));
    }
    // 124: a branch sluice lands on, submitted by name; to a branch that
    // takes pull requests, by the request from it (C.5).
    if let Some(named) = head {
        if takes_pulls(s, &branch)? {
            let mode = PullMode::Open {
                no_wait,
                subject: subject.as_deref(),
            };
            let named = SubmitHead::Branch(named);
            return submit_pull(
                s, host, pulls, clock, &named, &branch, mode, json, confirms, out,
            );
        }
        return submit(
            s,
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
                trace,
            },
            no_wait,
            json,
            confirms,
            out,
        );
    }
    if takes_pulls(s, &branch)? {
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
            s, host, pulls, clock, &worktree, &branch, mode, json, confirms, out,
        );
    }
    if status {
        return Ok(Submitted::Done(submitted(
            s, clock, &worktree, &branch, json, out,
        )?));
    }
    submit(
        s,
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
            trace,
        },
        no_wait,
        json,
        confirms,
        out,
    )
}

/// 152, record 0124 (design D10): a branch whose landing the forge
/// refused, held with the forge's words, and the ways out.
pub(crate) fn refused_landing_line(
    branch: &str,
    ids: &[u64],
    refused: &sluice_core::records::LandingRefused,
) -> String {
    let ids: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
    format!(
        "held: the forge refused the landing of {} on {branch} at {}: {}; withdraw one of \
         them or submit again from its place; each round pushes it again",
        ids.join(", "),
        refused.at,
        refused.words
    )
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
/// forecast forms from. Otherwise the forecast forms on every branch as the
/// host holds it since the host process's reading, fetching nothing; where
/// the host lacks what it forms from, it asks a reading afresh once. A
/// branch that current reading still brings no revision of is no branch,
/// refused as one (exit 2); anything else still lacking, or a branch
/// lacking where the reading was not current or a remote refused it, is a
/// read it could not make, exit 3 (design D16).
#[allow(clippy::too_many_arguments)]
fn forecast(
    here: &host_process::Here<'_, '_>,
    pushers: &dyn Pushers,
    worktree: &str,
    from: Option<&str>,
    branch: &str,
    after: &[u64],
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let s = here.s;
    // 64, 129 (the change `sluice-host-answers-every-read`, design D3):
    // `sluice host` forms it on the branch and its records as it holds
    // them after its current reading, fetching nothing.
    let formed = || -> Result<Result<Forecasted, SubmitRefusal>> {
        Ok(match from {
            Some(from) => s.forecast_branch(from, branch, after)?,
            None => s.forecast(worktree, branch, after)?,
        })
    };
    let got = match formed() {
        Err(e) if e.downcast_ref::<sluice_core::Unheld>().is_some() => {
            let reading = host_process::ask::read(here, true);
            let whole = reading.is_some_and(|r| r.current && r.refused.is_none());
            match formed() {
                Err(e) => match e.downcast_ref::<sluice_core::Unheld>() {
                    // 64: once a reading is current, a branch the host holds
                    // no revision of, at its own ref or a remote's, is none.
                    Some(sluice_core::Unheld::Branch(name)) if whole => match name == branch {
                        true => Err(SubmitRefusal::NoBranch),
                        false => Err(SubmitRefusal::NoHead(name.clone())),
                    },
                    Some(unheld) => {
                        return Err(anyhow::anyhow!(
                            "could not read what a forecast of {branch} forms from: {unheld}"
                        ))
                    }
                    None => return Err(e),
                },
                got => got?,
            }
        }
        got => got?,
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
    out: &mut dyn Write,
) -> Result<Submitted> {
    let accepted = match s.submit(&req)? {
        Ok(a) => a,
        Err(r) => {
            let named = head_name(s, &req.head);
            let text = refusal(&r, req.head.name(), &named, &req.branch, pushers)?;
            let next = refusal_next(&r, &req.branch);
            told::tell(out, json, Kind::Refused, &text, &next)?;
            return Ok(Submitted::Done(CONFLICT));
        }
    };
    let id = accepted.id;
    let copy = accepted.copy.as_ref();
    // 135: its entry written, the host process is told of it, and started
    // where none answers; the submit then returns or waits as asked.
    host_process::ask::serve(&host_process::Here {
        s,
        host,
        pulls,
        clock,
    });
    if json && no_wait {
        let mut doc =
            serde_json::json!({ "id": id, "branch": req.branch, "ahead": accepted.ahead });
        // ADR 0050: only a submission taken unchecked has any.
        if !accepted.warnings.is_empty() {
            doc["warnings"] = serde_json::json!(accepted.warnings);
        }
        writeln!(out, "{}", with_copy(doc, copy)?)?;
        return Ok(Submitted::Done(0));
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
        return Ok(Submitted::Done(0));
    }
    // The change `sluice-host-answers-every-read`, design D8: the wait
    // follows `sluice host`, which takes each of its steps. 124: a branch
    // named is handed over to nothing (record 0090).
    Ok(Submitted::Wait {
        ask: host_process::protocol::WaitAsk {
            target: id.to_string(),
            worktree: match &req.head {
                SubmitHead::Worktree(w) => Some(w.clone()),
                SubmitHead::Branch(_) => None,
            },
            json,
            confirm: confirms == Confirms::Every,
            confirm_landing: confirms == Confirms::Landing,
            own: Some(host_process::protocol::Followed {
                id,
                branch: req.branch.clone(),
            }),
            copy: copy.cloned(),
            ..Default::default()
        },
        queued: true,
    })
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
    out: &mut dyn Write,
) -> Result<Submitted> {
    let branch = &req.branch;
    let accepted = match s.submit_group(&req)? {
        Ok(a) => a,
        Err(refused) => {
            return Ok(Submitted::Done(refused_group(
                s, &req, &refused, pushers, json, out,
            )?))
        }
    };
    let names = req.names();
    let items: Vec<(&String, Accepted)> = names.iter().zip(accepted).collect();
    // 135: every entry written, the host process is told of them.
    host_process::ask::serve(&host_process::Here {
        s,
        host,
        pulls,
        clock,
    });
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
        return Ok(Submitted::Done(0));
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
        return Ok(Submitted::Done(0));
    }
    // The change `sluice-host-makes-each-change`, design D2: the wait on
    // each in turn, whose steps `sluice host` takes.
    Ok(Submitted::Wait {
        ask: host_process::protocol::WaitAsk {
            target: branch.clone(),
            json,
            group: Some(host_process::protocol::GroupWait {
                branch: branch.clone(),
                items: items
                    .iter()
                    .map(|(w, a)| host_process::protocol::GroupItem {
                        id: a.id,
                        worktree: w.to_string(),
                        copy: a.copy.clone(),
                        following: None,
                    })
                    .collect(),
            }),
            ..Default::default()
        },
        queued: true,
    })
}

/// 125: how the wait on an act of several worktrees ends, once each is
/// decided, or with `stopped` where it stopped before (the change
/// `sluice-host-makes-each-change`, design D2): each one's outcome in the
/// order given, the rest as they stand, and the exit the first's, in that
/// order, that did not land. Under `--json`, one document. Each ends with
/// what carried its work, where that was another (record 0100 D3).
fn group_end(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    group: &host_process::protocol::GroupWait,
    stopped: Option<i32>,
    out: &mut dyn Write,
) -> Result<i32> {
    let branch = &group.branch;
    let mut ended = Vec::new();
    for item in &group.items {
        let own = record(s, item.id, branch)?;
        let (sub, by) = match resolve(
            s,
            waiting,
            &own,
            item.following.as_ref(),
            &mut std::io::sink(),
        )? {
            Resolved::Decided(sub, by) => (sub, by),
            Resolved::Request(sub, _) => (sub, None),
            Resolved::Follow(read) => {
                let by = (read.id != own.id).then_some(CarriedBy::Entry(read));
                (own, by)
            }
        };
        ended.push((item, sub, by));
    }
    if waiting.json {
        let mut submissions = Vec::new();
        for (item, sub, by) in &ended {
            let mut record = with_carried(serde_json::to_value(sub)?, by.as_ref());
            record["worktree"] = serde_json::json!(item.worktree);
            submissions.push(with_copy(record, item.copy.as_ref())?);
        }
        let doc = serde_json::json!({ "branch": branch, "submissions": submissions });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        for (item, sub, by) in &ended {
            writeln!(out, "submission {} from {}:", sub.id, item.worktree)?;
            if sub.state.is_out() || sub.state == State::Refused {
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
    if let Some(code) = stopped {
        return Ok(code);
    }
    Ok(ended
        .iter()
        .map(|(_, sub, _)| exit_for(sub))
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
    clock: &'a dyn Clock,
    /// On a branch that takes pull requests: its pull requests, which carry
    /// a withdrawn submission's work on (137).
    pulls: Option<&'a dyn PullRequests>,
    json: bool,
    /// 124: how the host's copy differed from what the submit waited on
    /// submitted, carried by its `--json` document.
    copy: Option<&'a CopyDiffers>,
    /// 51: the worktree that waits, whose own outcome the wait hands over
    /// when it reports it; none for a wait that hands over nothing.
    worktree: Option<&'a str>,
    /// 51: whether its hand-over confirms on receipt (design D5).
    confirms: Confirms,
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

/// A wait's step, as `sluice host` takes it (the change
/// `sluice-host-answers-every-read`, design D8; the change
/// `sluice-host-makes-each-change`, design D2).
#[derive(Debug)]
pub enum WaitStepped {
    /// The wait ends with this exit, its report written and its hand-over
    /// recorded.
    Ended(i32),
    /// The wait follows the second, its own submission the first, or what
    /// carries its work.
    Follow(Submission, Submission),
    /// The wait follows a pull request, as this step left it.
    Pull(host_process::protocol::PullWait),
    /// The wait follows an act's pull requests, as this step left them.
    Pulls(host_process::protocol::PullGroupWait),
}

/// Where a wait on `own` stands, read from the records as the host holds
/// them (record 0100): decided, with what carried its work where that was
/// another; or what it follows, its own or what carries its work; or the
/// pull request that carries its work.
enum Resolved {
    Decided(Submission, Option<CarriedBy>),
    Follow(Submission),
    Request(Submission, u64),
}

/// Where the wait on `own` stands, reading `following` where it follows
/// what carries its work (record 0100 D1, D2): a decided submission it
/// reads ends the wait, or has it follow what carries its work on, read in
/// turn; a branch that took the work it follows ends it landed (122).
/// What it says of what carries its work goes to `out`.
fn resolve(
    s: &Sluice<'_>,
    waiting: &Waiting<'_>,
    own: &Submission,
    following: Option<&host_process::protocol::Followed>,
    out: &mut dyn Write,
) -> Result<Resolved> {
    let mut read = match following {
        Some(f) if (f.id, f.branch.as_str()) != (own.id, own.branch.as_str()) => {
            record(s, f.id, &f.branch)?
        }
        _ => own.clone(),
    };
    // Each carrier is read once: a chain of them ends.
    let mut seen = BTreeSet::new();
    loop {
        // 152, record 0124 (design D10): held, its end for the wait.
        if read.state == State::Refused {
            return Ok(Resolved::Decided(read, None));
        }
        if read.state.is_out() {
            match decided(s, waiting, own, read, out)? {
                Next::End(Waited::Decided(sub, by)) => return Ok(Resolved::Decided(sub, by)),
                Next::End(Waited::Request(sub, n)) => return Ok(Resolved::Request(sub, n)),
                Next::Follow(e) if e.state.is_out() && seen.insert(e.id) => {
                    read = e;
                    continue;
                }
                Next::Follow(e) => return Ok(Resolved::Follow(e)),
            }
        }
        // 122: the branch took the work it follows; record 0100 D2: what
        // it follows landed, its own end.
        return Ok(match landed(s, read.id, &read.branch) {
            Some(l) if l.id == own.id => Resolved::Decided(l, None),
            Some(l) => {
                let by = CarriedBy::Entry(l);
                let end = own_end(s, own.clone(), Some(&by))?;
                Resolved::Decided(end, Some(by))
            }
            None => Resolved::Follow(read),
        });
    }
}

/// One step of a wait, as `sluice host` answers `wait` (the change
/// `sluice-host-answers-every-read`, design D8; the change
/// `sluice-host-makes-each-change`, design D2): the wait's own
/// submission, found from its target where `ask` names none, and the
/// submission it follows, read from the records as the host holds them. A
/// decided submission ends the wait, its report written to `out` and its
/// outcome handed over to the waiting worktree, as `sluice wait` reported
/// and handed it over; a withdrawn one whose work goes on has the wait
/// follow what carries it (record 0100). A pull request it follows is read
/// once on the forge for each step, as is each of an act's; an act of
/// several worktrees is followed one in turn. A target, a number or a
/// branch, that names nothing, is refused as `sluice wait` refused it.
pub fn wait_step(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    ask: &host_process::protocol::WaitAsk,
    out: &mut dyn Write,
) -> Result<WaitStepped> {
    let json = ask.json;
    let named = ask.branch.as_deref();
    let worktree = ask.worktree.as_deref().unwrap_or(".");
    let confirms = Confirms::of(ask.confirm, ask.confirm_landing);
    // 125: an act's pull requests, each read once.
    if let Some(group) = &ask.pulls {
        let mut group = (**group).clone();
        if let Some(code) = ask.stopped {
            let code = pulls_end(s, &group, Some(code), false, json, out)?;
            return Ok(WaitStepped::Ended(code));
        }
        let stopped = pulls_once(s, pulls, clock, &mut group, false, json, out)?;
        if stopped.is_some() || group.items.iter().all(|i| i.decided.is_some()) {
            let code = pulls_end(s, &group, stopped, false, json, out)?;
            return Ok(WaitStepped::Ended(code));
        }
        return Ok(WaitStepped::Pulls(group));
    }
    // A pull request it follows, read once.
    if let Some(pull) = &ask.pull {
        let pull = (**pull).clone();
        if let Some(code) = ask.stopped {
            let code = pull_still(s, &pull, ask.copy.as_ref(), json, code, out)?;
            return Ok(WaitStepped::Ended(code));
        }
        return pull_step(s, host, pulls, clock, pull, ask, out);
    }
    let branch_of = |b: &str| takes_pulls(s, b).unwrap_or(false).then_some(pulls);
    // 125: an act of several worktrees, one in turn: the first not yet
    // decided is followed, and once each is, the act ends.
    if let Some(group) = &ask.group {
        let waiting = Waiting {
            clock,
            // A group of worktrees follows no pull request.
            pulls: None,
            json,
            copy: None,
            worktree: None,
            confirms: Confirms::No,
        };
        if let Some(code) = ask.stopped {
            let code = group_end(s, &waiting, group, Some(code), out)?;
            return Ok(WaitStepped::Ended(code));
        }
        for item in &group.items {
            let own = record(s, item.id, &group.branch)?;
            let mut said = Vec::new();
            if let Resolved::Follow(read) =
                resolve(s, &waiting, &own, item.following.as_ref(), &mut said)?
            {
                out.write_all(&said)?;
                return Ok(WaitStepped::Follow(own, read));
            }
        }
        let code = group_end(s, &waiting, group, None, out)?;
        return Ok(WaitStepped::Ended(code));
    }
    let own = match &ask.own {
        Some(o) => record(s, o.id, &o.branch)?,
        None => {
            let target = ask.target.as_str();
            let number = target.parse::<u64>();
            // Record 0130 D3: a pull request is into a branch, so its
            // number goes with a branch's name, never a submission's.
            if number.is_ok() && ask.pull_request.is_some() {
                writeln!(out, "sluice wait: {PULL_REQUEST_GOES_WITH_A_BRANCH}")?;
                return Ok(WaitStepped::Ended(USAGE));
            }
            if number.is_err() && named.is_some() {
                writeln!(out, "sluice wait: {}", branch_goes_with_a_number(target))?;
                return Ok(WaitStepped::Ended(USAGE));
            }
            match number {
                Ok(id) => match lookup(s, id, named)? {
                    Lookup::Found(sub) => sub,
                    Lookup::Unfound(unread) if unread.is_empty() => {
                        let (text, next) = no_such(id, named);
                        told::tell(out, json, Kind::NotFound, &text, &next)?;
                        return Ok(WaitStepped::Ended(FAILED));
                    }
                    Lookup::Unfound(unread) => {
                        return Ok(WaitStepped::Ended(unfound(id, &unread, json, out)?))
                    }
                    // A wait's 2 is a conflict: the refusal exits 64.
                    Lookup::Several(several) => {
                        let code = refuse_several(id, "sluice wait", &several, json, USAGE, out)?;
                        return Ok(WaitStepped::Ended(code));
                    }
                },
                // Record 0130 D2: a pull request into the branch the target
                // names, or the one the worktree's branch opened.
                Err(_) if ask.pull_request.is_some() || takes_pulls(s, target)? => {
                    return match pull_of(
                        s,
                        pulls,
                        clock,
                        target,
                        ask.pull_request,
                        worktree,
                        json,
                        out,
                    )? {
                        Ok(pull) => pull_step(s, host, pulls, clock, pull, ask, out),
                        Err(code) => Ok(WaitStepped::Ended(code)),
                    };
                }
                Err(_) => match last_submitted(s, worktree, target)? {
                    Ok(sub) => sub,
                    Err(from) => {
                        let text = format!("nothing submitted from {from} to {target}");
                        let next = [format!("sluice submit {target}")];
                        told::tell(out, json, Kind::NotFound, &text, &next)?;
                        return Ok(WaitStepped::Ended(FAILED));
                    }
                },
            }
        }
    };
    // A policy the wait cannot read is the host process's rounds' to say
    // (133): the wait follows its submission all the same.
    let waiting = Waiting {
        clock,
        pulls: branch_of(&own.branch),
        json,
        copy: ask.copy.as_ref(),
        worktree: ask.worktree.as_deref(),
        confirms,
    };
    match resolve(s, &waiting, &own, ask.following.as_ref(), out)? {
        Resolved::Decided(sub, by) => {
            let branch = sub.branch.clone();
            let code = report_delivering(
                s,
                waiting.worktree,
                sub,
                &branch,
                &BTreeMap::new(),
                by.as_ref(),
                waiting.copy,
                json,
                waiting.confirms,
                out,
            )?;
            Ok(WaitStepped::Ended(code))
        }
        Resolved::Follow(read) => Ok(WaitStepped::Follow(own, read)),
        // Withdrawn, while its pull request carries its work: the wait
        // follows the request for it (67, 137; record 0100 D4).
        Resolved::Request(sub, number) => {
            let pull = host_process::protocol::PullWait {
                branch: sub.branch.clone(),
                number,
                delivered_to: ask.worktree.clone(),
                follows_for: Some(sub),
                ..Default::default()
            };
            pull_step(s, host, pulls, clock, pull, ask, out)
        }
    }
}

/// One read of the pull request `pull` names, as a wait's step (design
/// D2): its end, or the request as the step left it; taken in as another
/// than its own, the wait on its own goes on as `sluice wait <id>` does
/// (record 0100 D4).
fn pull_step(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    mut pull: host_process::protocol::PullWait,
    ask: &host_process::protocol::WaitAsk,
    out: &mut dyn Write,
) -> Result<WaitStepped> {
    let worktree = ask.worktree.clone().unwrap_or_else(|| ".".into());
    let once = PullOnce {
        status: false,
        no_wait: false,
        worktree: &worktree,
        copy: ask.copy.as_ref(),
        json: ask.json,
        confirms: Confirms::of(ask.confirm, ask.confirm_landing),
    };
    match pull_once(s, pulls, clock, &mut pull, &once, out)? {
        Pulled::Ended(code) => Ok(WaitStepped::Ended(code)),
        Pulled::Open => Ok(WaitStepped::Pull(pull)),
        Pulled::Own(own) => {
            let ask = host_process::protocol::WaitAsk {
                own: Some(host_process::protocol::Followed {
                    id: own.id,
                    branch: own.branch.clone(),
                }),
                following: None,
                pull: None,
                ..ask.clone()
            };
            wait_step(s, host, pulls, clock, &ask, out)
        }
    }
}

/// What a wait does after a step of `sluice host`.
enum Stepped {
    Ended(i32),
    Follow(Submission, Submission),
    Pull(Box<host_process::protocol::PullWait>),
    Pulls(Box<host_process::protocol::PullGroupWait>),
}

/// A wait as a client of `sluice host` (the change
/// `sluice-host-answers-every-read`, design D8; the change
/// `sluice-host-makes-each-change`, design D2): it asks a step of `ask`,
/// then follows what the step names. It follows a submission on a
/// subscription of `folder` as `wait`, saying its state and its
/// combination's progress, its branch's hold and what each pass released,
/// from each document; where a document no longer holds what it follows,
/// or a round of its branch failed, it asks a step again. It follows a pull
/// request, or an act's, by asking a step at each of its ticks, which
/// `sluice host` answers by reading the forge, saying what each pass
/// released and its branch's hold between them. `sluice host` ends it,
/// with its report and its hand-over, or names what it follows next. Out
/// of time or stopped, it says what it waits on as it stands: a
/// submission as the last document held it, and a pull request or an act
/// as `sluice host` says it. It reads no record and no forge.
#[allow(clippy::too_many_arguments)]
pub fn follow_wait<'a>(
    process: &'a dyn host_process::HostProcess,
    here: Option<&'a host_process::Here<'a, 'a>>,
    folder: std::path::PathBuf,
    mut ask: host_process::protocol::WaitAsk,
    said_before: Option<State>,
    deadline: Option<chrono::DateTime<chrono::Utc>>,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    ask.deadline = deadline.map(|d| d.to_rfc3339());
    let mut waiter = subscriber::Waiter::new(process, here, folder.clone());
    let mut said_before = said_before;
    let mut stepped = wait_ask(process, here, &folder, &ask, out)?;
    loop {
        stepped = match stepped {
            Stepped::Ended(code) => return Ok(code),
            Stepped::Pull(pull) => {
                ask.pull = Some(pull);
                ask.pulls = None;
                follow_pull(
                    process,
                    here,
                    &folder,
                    &mut ask,
                    &mut waiter,
                    deadline,
                    clock,
                    pause,
                    out,
                )?
            }
            Stepped::Pulls(pulls) => {
                ask.pulls = Some(pulls);
                ask.pull = None;
                follow_pull(
                    process,
                    here,
                    &folder,
                    &mut ask,
                    &mut waiter,
                    None,
                    clock,
                    pause,
                    out,
                )?
            }
            Stepped::Follow(own, follows) => {
                ask.pull = None;
                ask.pulls = None;
                follow_submission(
                    process,
                    here,
                    &folder,
                    &mut ask,
                    &mut waiter,
                    (own, follows),
                    said_before.take(),
                    deadline,
                    clock,
                    pause,
                    out,
                )?
            }
        };
    }
}

/// Ask `sluice host` a step of `ask` from `folder`, and say what the step
/// said.
fn wait_ask(
    process: &dyn host_process::HostProcess,
    here: Option<&host_process::Here<'_, '_>>,
    folder: &std::path::Path,
    ask: &host_process::protocol::WaitAsk,
    out: &mut dyn Write,
) -> Result<Stepped> {
    use host_process::protocol::WaitStep;
    Ok(match process.wait(here, folder, ask)? {
        WaitStep::Ended(o) => {
            out.write_all(o.stdout.as_bytes())?;
            out.flush()?;
            for line in &o.stderr {
                telemetry::say(line);
            }
            Stepped::Ended(o.code)
        }
        WaitStep::Follow {
            own,
            follows,
            said,
            stderr,
        } => {
            out.write_all(said.as_bytes())?;
            stderr.iter().for_each(|l| telemetry::say(l));
            Stepped::Follow(own, follows)
        }
        WaitStep::Pull { pull, said, stderr } => {
            out.write_all(said.as_bytes())?;
            stderr.iter().for_each(|l| telemetry::say(l));
            Stepped::Pull(pull)
        }
        WaitStep::Pulls {
            pulls,
            said,
            stderr,
        } => {
            out.write_all(said.as_bytes())?;
            stderr.iter().for_each(|l| telemetry::say(l));
            Stepped::Pulls(pulls)
        }
    })
}

/// Follow the pull request, or the act's pull requests, that `ask` names,
/// one tick at a time (design D2): what each pass released and the
/// branch's hold said from the subscription, then a step, which reads the
/// forge in `sluice host`. Out of time or stopped, a last step says it as
/// it stands.
#[allow(clippy::too_many_arguments)]
fn follow_pull(
    process: &dyn host_process::HostProcess,
    here: Option<&host_process::Here<'_, '_>>,
    folder: &std::path::Path,
    ask: &mut host_process::protocol::WaitAsk,
    waiter: &mut subscriber::Waiter<'_>,
    deadline: Option<chrono::DateTime<chrono::Utc>>,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<Stepped> {
    let json = ask.json;
    let branch = match (&ask.pull, &ask.pulls) {
        (Some(p), _) => p.branch.clone(),
        (_, Some(g)) => g.branch.clone(),
        _ => ask.target.clone(),
    };
    // 121: the branch's hold as last said, so each is said once.
    let mut said_hold = None;
    let stop = |code: i32, ask: &mut host_process::protocol::WaitAsk, out: &mut dyn Write| {
        ask.stopped = Some(code);
        wait_ask(process, here, folder, ask, out)
    };
    if INTERRUPTED.load(Ordering::SeqCst) {
        return stop(STOPPED, ask, out);
    }
    if deadline.is_some_and(|d| clock.now() >= d) {
        return stop(WAITING, ask, out);
    }
    // 135: the host process runs the rounds; the wait follows it, says
    // what its passes released as a wait does, and says the branch's hold
    // as its reader's document has it (121).
    let reports = waiter.tick(clock);
    tell_released(&reports, &branch, json, out)?;
    if !json {
        tell_hold(
            out,
            &branch,
            hold_in(waiter.document(), &branch).as_ref(),
            &mut said_hold,
        )?;
    }
    if INTERRUPTED.load(Ordering::SeqCst) || !pause.pause() {
        return stop(STOPPED, ask, out);
    }
    wait_ask(process, here, folder, ask, out)
}

/// Follow the submission a step named, `followed`'s second, for its own,
/// the first, on the subscription's documents (the change
/// `sluice-host-answers-every-read`, design D8), until a step names
/// something else to follow or ends the wait.
#[allow(clippy::too_many_arguments)]
fn follow_submission(
    process: &dyn host_process::HostProcess,
    here: Option<&host_process::Here<'_, '_>>,
    folder: &std::path::Path,
    ask: &mut host_process::protocol::WaitAsk,
    waiter: &mut subscriber::Waiter<'_>,
    followed: (Submission, Submission),
    said_before: Option<State>,
    deadline: Option<chrono::DateTime<chrono::Utc>>,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<Stepped> {
    use host_process::protocol::Followed;
    let json = ask.json;
    let (mut own, mut follows) = followed;
    // What a step follows next is kept in the ask: the act's item's, or the
    // wait's own.
    let keep =
        |ask: &mut host_process::protocol::WaitAsk, own: &Submission, follows: &Submission| {
            let carrier = Followed {
                id: follows.id,
                branch: follows.branch.clone(),
            };
            match &mut ask.group {
                Some(group) => {
                    if let Some(item) = group.items.iter_mut().find(|i| i.id == own.id) {
                        item.following = (follows.id != own.id).then_some(carrier);
                    }
                }
                None => {
                    ask.own = Some(Followed {
                        id: own.id,
                        branch: own.branch.clone(),
                    });
                    ask.following = Some(carrier);
                }
            }
        };
    keep(ask, &own, &follows);
    let branch = own.branch.clone();
    type Moved = (State, Option<progress::Moved>);
    let mut said: Option<Moved> = said_before.map(|t| (t, None));
    let mut said_hold = None;
    let mut said_failed: Option<String> = None;
    let mut view: Option<BranchView> = None;
    let mut lacked: Option<serde_json::Value> = None;
    let mut first = true;
    // A step that names what to follow next: the same wait goes on, with
    // its act's next one said from where its submit left it.
    let next = |stepped: Stepped,
                own: &mut Submission,
                follows: &mut Submission,
                said: &mut Option<Moved>,
                ask: &mut host_process::protocol::WaitAsk|
     -> Option<Stepped> {
        match stepped {
            Stepped::Follow(o, f) => {
                if ask.group.is_some() && o.id != own.id {
                    *said = Some((State::Queued, None));
                }
                *own = o;
                *follows = f;
                keep(ask, own, follows);
                None
            }
            other => Some(other),
        }
    };
    waiter.look();
    loop {
        // The document: the submission it follows as it holds it, or a
        // step where it holds it no longer.
        if let Some(doc) = waiter
            .document()
            .filter(|d| d["branches"].is_array())
            .cloned()
        {
            view = branch_in(&doc, &branch);
            let held = view
                .as_ref()
                .and_then(|v| v.submissions.iter().find(|x| x.id == follows.id))
                .filter(|x| sluice_core::home::same_work(x, &follows))
                .cloned();
            // 152, record 0124: held, as a decided one, is a step's to end.
            let decided = held
                .as_ref()
                .is_some_and(|x| x.state == State::Refused || x.state.is_out());
            match held {
                Some(x) if !decided => {
                    if (x.id, &x.branch) == (own.id, &own.branch) {
                        own = x.clone();
                    }
                    follows = x;
                }
                _ if lacked.as_ref() != Some(&doc) => {
                    lacked = Some(doc);
                    let stepped = wait_ask(process, here, folder, ask, out)?;
                    if let Some(other) = next(stepped, &mut own, &mut follows, &mut said, ask) {
                        return Ok(other);
                    }
                    continue;
                }
                _ => {}
            }
        }
        let by = (follows.id != own.id).then(|| CarriedBy::Entry(follows.clone()));
        let id = follows.id;
        if !json {
            let combination = view.as_ref().and_then(|v| progress::own(v, id));
            let now: Moved = (follows.state, combination.map(progress::moved));
            if said.as_ref() != Some(&now) {
                match combination {
                    Some(c) => {
                        let line = progress::testing_line(&branch, c, clock.now(), None);
                        writeln!(out, "{line}")?;
                    }
                    None => match detail(&branch, &follows, None, None, &mut BTreeSet::new()) {
                        d if d.is_empty() => writeln!(out, "#{id} {}", follows.state.word())?,
                        d => writeln!(out, "#{id} {} · {d}", follows.state.word())?,
                    },
                }
                said = Some(now);
            }
            if let Some(v) = &view {
                tell_hold(out, &branch, v.held.as_ref(), &mut said_hold)?;
            }
        }
        // As it stands when its time ran out or it was stopped: its own
        // record as the last document held it; an act as `sluice host`
        // says it.
        let undecided = |code: i32,
                         ask: &mut host_process::protocol::WaitAsk,
                         out: &mut dyn Write|
         -> Result<Stepped> {
            if ask.group.is_some() {
                ask.stopped = Some(code);
                return wait_ask(process, here, folder, ask, out);
            }
            if json {
                let doc = with_copy(serde_json::to_value(&own)?, ask.copy.as_ref())?;
                let mut doc = with_carried(doc, by.as_ref());
                if let Some(c) = view.as_ref().and_then(|v| progress::own(v, id)) {
                    under_test(&mut doc, c);
                }
                writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
            } else {
                let again = asked("sluice wait", own.id, ask.branch.as_deref());
                writeln!(out, "still waiting: {again}")?;
            }
            Ok(Stepped::Ended(code))
        };
        // Its first pass is the host process's whatever its deadline (1).
        if !first {
            if deadline.is_some_and(|d| clock.now() >= d) {
                return undecided(WAITING, ask, out);
            }
            if !pause.pause() {
                return undecided(STOPPED, ask, out);
            }
        }
        let reports = match first {
            true => waiter.first(clock, host_process::read_for()),
            false => waiter.tick(clock),
        };
        first = false;
        tell_released(&reports, &branch, json, out)?;
        if INTERRUPTED.load(Ordering::SeqCst) {
            return undecided(STOPPED, ask, out);
        }
        let failed = reports
            .into_iter()
            .filter_map(|r| r.failed.get(&branch).cloned())
            .next_back();
        if let Some(words) = failed {
            // 122: the branch may have taken the work it follows, and what
            // failed came after: a step says.
            let stepped = wait_ask(process, here, folder, ask, out)?;
            if let Some(other) = next(stepped, &mut own, &mut follows, &mut said, ask) {
                return Ok(other);
            }
            if said_failed.as_deref() != Some(words.as_str()) {
                telemetry::warn(&format!("sluice: {branch}: {words}"));
                said_failed = Some(words);
            }
        }
    }
}

/// 51, D8: a submission's document, its record as it is, with, for one
/// landed, failed or conflicting, its `notice`: the words the round pushes
/// into its session and `sluice inbox` gives it, from `decided` where the
/// round's outbox item names its outcome, else from the record's own. One
/// whose landing the forge refused has `refused`, the forge's `words` and
/// when (`at`), read from the branch's held landing, and the notice of
/// that refusal, which decides no outcome (152, record 0124). A withdrawn,
/// queued or testing one has none. The one document `sluice inbox
/// --json`, `sluice status <branch> [--all] --json`, `sluice status <n>
/// --json`, `sluice wait --json` and a waiting submit give a submission: a
/// contract a caller reads, which only gains fields.
fn decided_document(
    s: &Sluice<'_>,
    sub: &Submission,
    decided: Option<&Decided>,
) -> Result<serde_json::Value> {
    let refusal = match sub.state {
        State::Refused => landing_refusal(s, &sub.branch, sub.id)?,
        _ => None,
    };
    document(sub, decided, refusal.as_ref())
}

/// [`decided_document`], `refusal` the forge's refusal of the landing that
/// holds `sub`, as its branch's records keep it.
fn document(
    sub: &Submission,
    decided: Option<&Decided>,
    refusal: Option<&sluice_core::records::LandingRefused>,
) -> Result<serde_json::Value> {
    let mut doc = serde_json::to_value(sub)?;
    let refused = refusal.map(|r| Decided::Refused {
        words: r.words.clone(),
    });
    if let Some(r) = refusal {
        doc["refused"] = serde_json::json!({ "words": r.words, "at": r.at });
    }
    let own = sub.decided().or(refused);
    if let Some(d) = decided.or(own.as_ref()) {
        doc["notice"] = sluice_core::notice(sub.id, &sub.branch, d).into();
    }
    Ok(doc)
}

/// 49, 69 (design D17): `doc`, `sub`'s record, with `checks`, a row for
/// each required check of the combination it was last tested in, as its
/// report gives them, read from the results of that combination alone. It
/// decides nothing, so a read that fails adds nothing, and says so.
fn with_checks(s: &Sluice<'_>, sub: &Submission, doc: &mut serde_json::Value) {
    match s.verdict_of(&sub.branch, sub.id) {
        Ok(Some(v)) if !v.rows.is_empty() => doc["checks"] = serde_json::json!(v.rows),
        Ok(_) => {}
        Err(e) => telemetry::warn(&format!(
            "sluice: warning: could not read #{}'s checks: {e:#}",
            sub.id
        )),
    }
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
    /// Landed, failed, conflict or withdrawn: whatever the host process's
    /// round failed after a landing, it is landed (122; rule core.48).
    Decided(Submission, Option<CarriedBy>),
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
            Next::End(Waited::Decided(end, Some(by)))
        }
        WorkGoes::Outcome(o, _) if o.id == own.id => Next::End(Waited::Decided(o, None)),
        // What carried its work ended as its request closed: the request.
        WorkGoes::Outcome(_, Some(n)) => {
            let by = CarriedBy::Request(n, RequestEnd::Closed);
            let end = own_end(s, own.clone(), Some(&by))?;
            Next::End(Waited::Decided(end, Some(by)))
        }
        WorkGoes::Outcome(o, None) => {
            let by = CarriedBy::Entry(o);
            let end = own_end(s, own.clone(), Some(&by))?;
            Next::End(Waited::Decided(end, Some(by)))
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

/// 97, 123: what the host process's passes in `reports` freed, and what
/// their rounds of `branch` released, as a waiting command's own rounds
/// said them: beside its lines, or on stderr under `--json`.
fn tell_released(
    reports: &[host_process::pass::Report],
    branch: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<()> {
    for r in reports {
        let lines = r
            .freed
            .iter()
            .chain(r.released.get(branch).into_iter().flatten());
        for line in lines {
            if json {
                telemetry::say(line);
            } else {
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(())
}

/// 121: `branch`'s hold, as the last document `sluice host` sent the
/// wait has it; `None` where it has none or none came.
fn hold_in(doc: Option<&serde_json::Value>, branch: &str) -> Option<BranchHold> {
    branch_in(doc?, branch)?.held
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
    // 152, record 0124 (design D10): held, with the forge's refusal.
    let refusal = match sub.state {
        State::Refused => landing_refusal(s, branch, id)?,
        _ => None,
    };
    if json {
        // The submission's record as `sluice status <n> --json` gives it,
        // its notice and its refusal among it (51, 152).
        let doc = document(sub, None, refusal.as_ref())?;
        let mut doc = with_carried(with_copy(doc, copy)?, by);
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
            // 139: what the sandbox refused a run of it, beside the fails,
            // which stand (design D1 of the change
            // `the-gate-sandbox-lets-the-tools-write`), and, design D8, a
            // run whose executor could not read it, read as refused nothing.
            let mut said: Vec<String> = Vec::new();
            for l in &combination_log {
                if let Event::Detail {
                    refusals_unread,
                    refused,
                    ..
                } = &l.event
                {
                    let lines = refused.iter().cloned().chain(
                        refusals_unread
                            .iter()
                            .map(|why| words::refusals_unread(why)),
                    );
                    for line in lines {
                        if !said.contains(&line) {
                            said.push(line);
                        }
                    }
                }
            }
            for line in said {
                writeln!(out, "  {line}")?;
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
        // 152, record 0124 (design D10): held, with the forge's words.
        State::Refused => {
            let words = refusal.map(|r| r.words).unwrap_or_default();
            writeln!(out, "held: the forge refused its landing: {words}")?;
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
                ways.push(words::flaky_way_on(&f.flaky));
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
                let mut doc = decided_document(s, &o.submission, Some(&o.decided))?;
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
/// 137 (design D22): the refusal of a branch that takes pull requests on
/// records each host keeps, where `e` is one.
fn no_intake(e: &anyhow::Error) -> Option<&sluice_core::policy::NoIntake> {
    e.chain()
        .find_map(|c| c.downcast_ref::<sluice_core::policy::NoIntake>())
}

fn takes_pulls(s: &Sluice<'_>, branch: &str) -> Result<bool> {
    // 129 (design D16): a sluice that reads held fetches nothing for it.
    let Some(tip) = s.branch_read(branch)? else {
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
/// Record 0130 D3: `--pull-request` given a submission's number.
const PULL_REQUEST_GOES_WITH_A_BRANCH: &str =
    "--pull-request names a pull request into a branch, as sluice wait <branch> --pull-request <n>";

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
}

/// The wait that follows pull request `n` into `branch` from any folder of
/// the repository, on any host: the one handle a request's work keeps
/// from the submit to its landing (record 0130 D1; rule core.48).
fn pull_wait(branch: &str, n: u64) -> String {
    format!("sluice wait {branch} --pull-request {n}")
}

/// What a submit made, as `sluice host` answers it (the change
/// `sluice-host-makes-each-change`, design D2): its exit where it ends,
/// or, for a waiting submit, the wait it follows next.
pub enum Submitted {
    /// It ends with this exit.
    Done(i32),
    /// It waits as `ask` says; `queued` where it said its submission
    /// queued already.
    Wait {
        ask: host_process::protocol::WaitAsk,
        queued: bool,
    },
}

/// `sluice submit` to a branch with pull requests (C.5 pull request):
/// push the worktree's branch and open or update its pull request, which
/// waits for its conditions, and take it in, as the submit's own first
/// pass; then, unless it does not wait, the wait that follows it, whose
/// steps `sluice host` takes (the change `sluice-host-makes-each-change`,
/// design D2). [`PullMode::Status`] only says where it stands. A branch
/// named (124) opens the request from that branch, reading no worktree,
/// and its outcome is handed over to none: the request carries it (C.5;
/// record 0090). A waiting submit takes as its own the first submission of
/// the request it reads; once the request is taken in as another, it waits
/// on its own as `sluice wait <id>` does (record 0100 D4).
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
    out: &mut dyn Write,
) -> Result<Submitted> {
    let seams = s.seams();
    // The place the request is found by, and the outcome delivered to: a
    // worktree's; none for a branch named, whose way is the request's.
    let worktree = head.name();
    let delivered_to = match head {
        SubmitHead::Worktree(w) => Some(w.clone()),
        SubmitHead::Branch(_) => None,
    };
    let status = matches!(mode, PullMode::Status);
    let no_wait = matches!(mode, PullMode::Open { no_wait: true, .. });
    // 124: how the host's copy differed from what the submit opened.
    let mut copy = None;
    let number = match mode {
        PullMode::Status => match pulls.find(seams, clock, branch, worktree)? {
            Some(n) => n,
            None => {
                let text = format!("no pull request from {worktree} into {branch}");
                let next = [format!("sluice submit {branch}")];
                told::tell(out, json, Kind::NotFound, &text, &next)?;
                return Ok(Submitted::Done(1));
            }
        },
        PullMode::Open { subject, .. } => match pulls.open(seams, clock, head, branch, subject)? {
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
                return Ok(Submitted::Done(CONFLICT));
            }
        },
    };
    // 135: the first pass's intake is a submit's own, and `--status`'s:
    // the answer names what it took in. Every other is sluice's host
    // process's. The user, 2026-10-28: a submit that does not wait fails at
    // once on a spent allowance, naming when it ends (134).
    let intaken = intake(pulls, s, clock, branch, json, out);
    intaken.say(branch);
    if let (Some(e), true) = (intaken.limited, no_wait) {
        return Err(e);
    }
    // 135: the host process is told of the request it takes in.
    if !status {
        host_process::ask::serve(&host_process::Here {
            s,
            host,
            pulls,
            clock,
        });
    }
    let mut pull = host_process::protocol::PullWait {
        branch: branch.to_string(),
        number,
        delivered_to,
        takes_own: !status && !no_wait,
        ..Default::default()
    };
    let once = PullOnce {
        status,
        no_wait,
        worktree,
        copy: copy.as_ref(),
        json,
        confirms,
    };
    let wait = |pull: host_process::protocol::PullWait| host_process::protocol::WaitAsk {
        target: branch.to_string(),
        worktree: pull.delivered_to.clone(),
        json,
        confirm: confirms == Confirms::Every,
        confirm_landing: confirms == Confirms::Landing,
        copy: copy.clone(),
        pull: Some(Box::new(pull)),
        ..Default::default()
    };
    Ok(match pull_once(s, pulls, clock, &mut pull, &once, out)? {
        Pulled::Ended(code) => Submitted::Done(code),
        Pulled::Open => Submitted::Wait {
            ask: wait(pull),
            queued: false,
        },
        Pulled::Own(own) => Submitted::Wait {
            ask: host_process::protocol::WaitAsk {
                own: Some(host_process::protocol::Followed {
                    id: own.id,
                    branch: own.branch.clone(),
                }),
                pull: None,
                ..wait(pull)
            },
            queued: false,
        },
    })
}

/// How a step of a pull request reads it, beside its state.
struct PullOnce<'a> {
    /// `--status`: only say where it stands.
    status: bool,
    /// A submit that does not wait: say where it stands, once.
    no_wait: bool,
    /// The worktree, or the branch named, the request is from.
    worktree: &'a str,
    copy: Option<&'a CopyDiffers>,
    json: bool,
    confirms: Confirms,
}

/// What one read of a pull request a wait follows came to.
enum Pulled {
    /// The wait ends with this exit, its report written.
    Ended(i32),
    /// It stands open and undecided: the wait follows it on.
    Open,
    /// It was taken in as another submission than its own: the wait follows
    /// its own as `sluice wait <id>` does (record 0100 D4).
    Own(Submission),
}

/// One read of the pull request `pull` names, as a step of the wait that
/// follows it, its state kept in `pull` for the next (the change
/// `sluice-host-makes-each-change`, design D2): each state it reaches is
/// said, once, and an outcome is reported, handed over to the worktree
/// whose branch opened it, as [`await_submission`] hands one over (51). A
/// read the forge could not answer for the retry policy's time ends the
/// wait as it could not read (134).
fn pull_once(
    s: &Sluice<'_>,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    pull: &mut host_process::protocol::PullWait,
    once: &PullOnce<'_>,
    out: &mut dyn Write,
) -> Result<Pulled> {
    let seams = s.seams();
    let json = once.json;
    let (status, no_wait) = (once.status, once.no_wait);
    let copy = once.copy;
    let worktree = once.worktree;
    let branch = pull.branch.clone();
    let branch = branch.as_str();
    let number = pull.number;
    let delivered_to = pull.delivered_to.clone();
    let delivered_to = delivered_to.as_deref();
    let follows_for = pull.follows_for.clone();
    let follows_for = follows_for.as_ref();
    let what = format!("#{number}");
    // Record 0130 D4: the wait that follows it by its number, from any
    // folder; `sluice wait <id>` for a wait that follows it for submission
    // `<id>`.
    let again = match follows_for {
        Some(own) => format!("sluice wait {}", own.id),
        None => pull_wait(branch, number),
    };
    let followed = match pulls.follow(seams, clock, branch, number) {
        Ok(f) => f,
        Err(e) if no_wait || status => return Err(e),
        Err(e) if waited_out(&e) => return Ok(Pulled::Ended(WAITING)),
        Err(e) => {
            let code = could_not_read(&unread(e)?, &what, &again, json, out)?;
            return Ok(Pulled::Ended(code));
        }
    };
    match followed {
        Followed::Done(sub, names) => {
            if status && json {
                let doc = with_sends(serde_json::to_value(&sub)?, s, worktree)?;
                writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
                return Ok(Pulled::Ended(0));
            }
            if status {
                let status = s.status(branch)?;
                let names = status_names(&status, std::slice::from_ref(&sub));
                let testing = progress::lines(s, branch, Some(sub.id), clock.now(), Some(&names))?;
                write_status(
                    &status,
                    None,
                    &testing,
                    std::slice::from_ref(&sub),
                    &names,
                    None,
                    out,
                )?;
                return Ok(Pulled::Ended(0));
            }
            // Record 0100 D2, D4: a wait that follows the request for its
            // own ends with its own, the submission decided what carried
            // its work, or the request where it is its own.
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
                let code = report_delivering(
                    s,
                    delivered_to,
                    end,
                    branch,
                    &names,
                    Some(&by),
                    copy,
                    json,
                    once.confirms,
                    out,
                )?;
                return Ok(Pulled::Ended(code));
            }
            match &pull.own {
                Some(own) if pull.takes_own && own.id != sub.id => {
                    return Ok(Pulled::Own(own.clone()));
                }
                _ => {}
            }
            // 51: the request was opened from this worktree's branch, so
            // the outcome a waiting submit reports is its own; a branch
            // named's is no one's (124).
            let code = report_delivering(
                s,
                delivered_to,
                sub,
                branch,
                &names,
                None,
                copy,
                json,
                once.confirms,
                out,
            )?;
            return Ok(Pulled::Ended(code));
        }
        // 124, 137: `--status` says where it stands, and exits 0, as it
        // does where this host takes the request in; a wait and a waiting
        // submit report the outcome with its exit (record 0096).
        f @ (Followed::Landed { .. } | Followed::SentBack { .. }) if status => {
            let code = across_status(s, worktree, number, branch, &f, copy, json, out)?;
            return Ok(Pulled::Ended(code));
        }
        // Record 0100 D2: a wait that follows the request for its own ends
        // with its own, landed where the branch holds its head.
        f @ (Followed::Landed { .. } | Followed::SentBack { .. } | Followed::Closed)
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
            let code = report_delivering(
                s,
                delivered_to,
                own,
                branch,
                &BTreeMap::new(),
                Some(&by),
                copy,
                json,
                once.confirms,
                out,
            )?;
            return Ok(Pulled::Ended(code));
        }
        f @ (Followed::Landed { .. } | Followed::SentBack { .. }) => {
            return Ok(Pulled::Ended(across(number, branch, &f, copy, json, out)?));
        }
        Followed::Closed => {
            if json {
                let doc = serde_json::json!({
                    "pull_request": number, "branch": branch, "state": "closed",
                });
                writeln!(out, "{}", with_copy(doc, copy)?)?;
            } else {
                writeln!(out, "#{number} was closed without landing")?;
            }
            return Ok(Pulled::Ended(FAILED));
        }
        Followed::Open {
            title,
            lines,
            taken_in_by,
            queue,
            submission,
            names,
            head,
            draft,
        } => {
            // 137, design D5: where another host takes it in, the host the
            // mark names and the queue its check carries, said again only
            // when it changes.
            pull.there = pulls
                .elsewhere(branch)
                .then_some(There { taken_in_by, queue });
            if let (false, Some(t)) = (json, &pull.there) {
                let line = t.line(number, branch);
                if pull.said_there.as_ref() != Some(&line) {
                    writeln!(out, "{line}")?;
                    pull.said_there = Some(line);
                }
            }
            // Taken in on the head just pushed: the submission is the
            // answer, as a no-wait submit's on any branch, with the pull
            // request beside it.
            if let (true, Some(sub)) = (no_wait, &submission) {
                let ahead: Vec<u64> = s
                    .status(branch)?
                    .submissions
                    .iter()
                    .filter(|x| x.id < sub.id && !x.state.is_out())
                    .map(|x| x.id)
                    .collect();
                if json {
                    // Record 0130 D4: the wait that follows the request
                    // from any folder, beside the submission.
                    let doc = serde_json::json!({
                        "id": sub.id, "branch": branch, "ahead": ahead,
                        "pull_request": number, "wait": pull_wait(branch, number),
                    });
                    writeln!(out, "{}", with_copy(doc, copy)?)?;
                } else {
                    let name = |i: u64| names.get(&i).cloned().unwrap_or_else(|| format!("#{i}"));
                    let ahead = queued_ahead(&ahead, name);
                    writeln!(
                        out,
                        "submission {} to {branch} from #{number} · {ahead}",
                        sub.id
                    )?;
                    writeln!(out, "wait: {}", pull_wait(branch, number))?;
                }
                return Ok(Pulled::Ended(0));
            }
            // Record 0100 D4: its own is the first submission of the
            // request it reads; taken in as another, it waits on its own.
            if let (true, Some(e)) = (pull.takes_own, &submission) {
                match &pull.own {
                    None => pull.own = Some(e.clone()),
                    Some(own) if own.id != e.id => return Ok(Pulled::Own(own.clone())),
                    Some(_) => {}
                }
            }
            let now = (title, lines);
            if pull.last.as_ref() != Some(&now) {
                if json && status {
                    // Its head and draft mark, for a driver to tell
                    // whether it stands at what the worktree would send
                    // (record 0065).
                    let doc = serde_json::json!({
                        "pull_request": number, "branch": branch, "state": now.0,
                        "head": head, "draft": draft, "wait": pull_wait(branch, number),
                    });
                    let doc = with_sends(There::with(doc, pull.there.as_ref())?, s, worktree)?;
                    writeln!(out, "{}", with_copy(doc, copy)?)?;
                } else if json && no_wait {
                    let doc = serde_json::json!({
                        "pull_request": number, "branch": branch, "state": now.0,
                        "wait": pull_wait(branch, number),
                    });
                    let doc = There::with(doc, pull.there.as_ref())?;
                    writeln!(out, "{}", with_copy(doc, copy)?)?;
                } else if !json {
                    writeln!(out, "{}", now.0)?;
                    for l in &now.1 {
                        writeln!(out, "  {l}")?;
                    }
                }
                pull.last = Some(now);
            }
            pull.taken = submission.map(|x| x.id);
        }
    }
    if no_wait || status {
        // Record 0130 D4: the request still open and undecided, the wait
        // that follows it, on a line of its own after its state.
        if !json {
            writeln!(out, "wait: {}", pull_wait(branch, number))?;
        }
        return Ok(Pulled::Ended(0));
    }
    Ok(Pulled::Open)
}

/// A wait on the pull request `pull` names that stops with `code`, out of
/// time or stopped (the change `sluice-host-makes-each-change`, design
/// D2): taken in, its submission's record as it stands now, as `sluice
/// wait <id>` ends; otherwise the document `--status` prints, or where to
/// read it again. A wait that follows the request for its own ends with
/// its own record, and what carries its work as it stands (record 0100
/// D2).
fn pull_still(
    s: &Sluice<'_>,
    pull: &host_process::protocol::PullWait,
    copy: Option<&CopyDiffers>,
    json: bool,
    code: i32,
    out: &mut dyn Write,
) -> Result<i32> {
    let (branch, number) = (pull.branch.as_str(), pull.number);
    let sub = match pull.taken {
        Some(id) => s.state_on(branch, id)?,
        None => None,
    };
    if let Some(own) = &pull.follows_for {
        let by = match sub {
            Some(e) => CarriedBy::Entry(e),
            None => CarriedBy::Request(number, RequestEnd::Open),
        };
        if json {
            let doc = with_copy(serde_json::to_value(own)?, copy)?;
            let doc = with_carried(doc, Some(&by));
            writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
        } else {
            writeln!(out, "still waiting: sluice wait {}", own.id)?;
        }
        return Ok(code);
    }
    if let Some(sub) = sub {
        if json {
            writeln!(out, "{}", record_json(&sub, copy)?)?;
        } else {
            writeln!(out, "still waiting: sluice wait {}", sub.id)?;
        }
    } else if json {
        let state = pull.last.as_ref().map_or("waiting", |l| l.0.as_str());
        let doc = serde_json::json!({
            "pull_request": number, "branch": branch, "state": state,
            "wait": pull_wait(branch, number),
        });
        let doc = There::with(doc, pull.there.as_ref())?;
        writeln!(out, "{}", with_copy(doc, copy)?)?;
    } else {
        writeln!(out, "still open: #{number}; {}", pull_wait(branch, number))?;
    }
    Ok(code)
}

/// The pull request a wait on `target` follows where it names no
/// submission (the change `sluice-host-makes-each-change`, design D2): pull
/// request `number` where the wait names one, from any folder, its outcome
/// still handed over to the worktree the wait runs for (record 0130 D2,
/// D3); else the one the worktree's branch opened. `Err` with the wait's
/// exit, its words written, where it follows none.
fn pull_of(
    s: &Sluice<'_>,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    target: &str,
    number: Option<u64>,
    worktree: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<std::result::Result<host_process::protocol::PullWait, i32>> {
    let found = match number {
        // Record 0130 D3: a branch that takes no pull requests, or a forge
        // that holds no such one into it, follows nothing.
        Some(n) => match takes_pulls(s, target)? {
            false => Ok(None),
            true => pulls
                .holds(s.seams(), clock, target, n)
                .map(|held| held.then_some(n)),
        },
        None => pulls.find(s.seams(), clock, target, worktree),
    };
    let found = match found {
        Ok(found) => found,
        Err(e) => {
            if waited_out(&e) {
                return Ok(Err(WAITING));
            }
            let (what, again) = match number {
                Some(n) => (format!("#{n}"), pull_wait(target, n)),
                None => (
                    format!("the pull request from {worktree} into {target}"),
                    format!("sluice wait {target}"),
                ),
            };
            return Ok(Err(could_not_read(&unread(e)?, &what, &again, json, out)?));
        }
    };
    let Some(n) = found else {
        let (text, next) = match number {
            Some(n) => (format!("no pull request #{n} into {target}"), vec![]),
            None => (
                format!("no pull request from {worktree} into {target}"),
                vec![format!("sluice submit {target}")],
            ),
        };
        told::tell(out, json, Kind::NotFound, &text, &next)?;
        return Ok(Err(FAILED));
    };
    Ok(Ok(host_process::protocol::PullWait {
        branch: target.to_string(),
        number: n,
        delivered_to: Some(worktree.to_string()),
        takes_own: true,
        ..Default::default()
    }))
}

/// 125: `sluice submit` of several worktrees to a branch with pull
/// requests: every worktree's branch pushed and its pull request opened or
/// updated, each marked one of the group, or none when any worktree is
/// refused; then every pull request marked ready and queued, each
/// submission said after its pull request, or none when any falls short
/// of its conditions, each such named. It takes them in, as its own first
/// pass, and says where each stands; then, unless `no_wait`, the wait that
/// follows each, whose steps `sluice host` takes until every one is
/// decided, and which reports each one's outcome in the order given (the
/// change `sluice-host-makes-each-change`, design D2).
#[allow(clippy::too_many_arguments)]
fn submit_pull_group(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    req: GroupRequest,
    no_wait: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<Submitted> {
    let seams = s.seams();
    let branch = &req.branch;
    let names = req.names();
    let opened = match pulls.open_group(seams, clock, &names, &req.subjects, branch)? {
        Ok(o) => o,
        Err(GroupRefused::Worktrees(refused)) => {
            return Ok(Submitted::Done(refused_group(
                s,
                &req,
                &refused,
                &(),
                json,
                out,
            )?))
        }
        Err(GroupRefused::Short { opened, short }) => {
            return Ok(Submitted::Done(short_group(
                &req, &opened, &short, json, out,
            )?))
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
    let mut group = host_process::protocol::PullGroupWait {
        branch: branch.clone(),
        told: None,
        items: names
            .iter()
            .zip(&opened)
            .map(|(w, o)| host_process::protocol::PullItem {
                number: o.number,
                worktree: w.clone(),
                copy: o.copy.clone(),
                id: o.submission.as_ref().map(|a| a.id),
                ahead: o
                    .submission
                    .as_ref()
                    .map(|a| a.ahead.clone())
                    .unwrap_or_default(),
                decided: None,
                last: None,
                there: None,
                said_there: None,
            })
            .collect(),
    };
    // 135: as `submit_pull`, the first pass's intake the submit's own,
    // every other sluice's host process's.
    let intaken = intake(pulls, s, clock, branch, json, out);
    intaken.say(branch);
    if let (Some(e), true) = (intaken.limited, no_wait) {
        return Err(e);
    }
    // 135: the host process is told of the requests it takes in.
    host_process::ask::serve(&host_process::Here {
        s,
        host,
        pulls,
        clock,
    });
    let stopped = pulls_once(s, pulls, clock, &mut group, no_wait, json, out)?;
    if no_wait || stopped.is_some() || group.items.iter().all(|i| i.decided.is_some()) {
        return Ok(Submitted::Done(pulls_end(
            s, &group, stopped, no_wait, json, out,
        )?));
    }
    Ok(Submitted::Wait {
        ask: host_process::protocol::WaitAsk {
            target: branch.clone(),
            json,
            pulls: Some(Box::new(group)),
            ..Default::default()
        },
        queued: false,
    })
}

/// One read of each pull request of `group` not yet decided, as a step of
/// the wait that follows them (125; the change
/// `sluice-host-makes-each-change`, design D2): each state it reaches said
/// once, and each decided kept. `Some` with the wait's exit where a read
/// the forge could not answer for the retry policy's time stops it, said on
/// stderr.
fn pulls_once(
    s: &Sluice<'_>,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    group: &mut host_process::protocol::PullGroupWait,
    no_wait: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<Option<i32>> {
    let seams = s.seams();
    let branch = group.branch.clone();
    let branch = branch.as_str();
    let what = hashes(
        &group.items.iter().map(|o| o.number).collect::<Vec<_>>(),
        None,
        &mut BTreeSet::new(),
    );
    let again = format!("sluice wait {branch}");
    for o in group.items.iter_mut() {
        if o.decided.is_some() {
            continue;
        }
        let followed = match pulls.follow(seams, clock, branch, o.number) {
            Ok(f) => f,
            Err(e) if no_wait => return Err(e),
            Err(e) => {
                let e = unread(e)?;
                let code = could_not_read(&e, &what, &again, false, &mut std::io::sink())?;
                return Ok(Some(code));
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
                o.there = pulls
                    .elsewhere(branch)
                    .then_some(There { taken_in_by, queue });
                if let (false, Some(t)) = (json, &o.there) {
                    let line = t.line(o.number, branch);
                    if o.said_there.as_ref() != Some(&line) {
                        writeln!(out, "{line}")?;
                        o.said_there = Some(line);
                    }
                }
                let now = (title, lines);
                if o.last.as_ref() != Some(&now) {
                    if !json {
                        writeln!(out, "#{} {}", o.number, now.0)?;
                        for l in &now.1 {
                            writeln!(out, "  {l}")?;
                        }
                    }
                    o.last = Some(now);
                }
                // Record 0130 D4: still open and undecided, the wait that
                // follows it, after its state.
                if no_wait && !json {
                    writeln!(out, "wait: {}", pull_wait(branch, o.number))?;
                }
            }
            followed => o.decided = Some(followed),
        }
    }
    Ok(None)
}

/// How the wait on `group` ends, with `stopped` where it stopped before
/// every one was decided (125): under `--json`, one document of every pull
/// request as it stands; then, unless the submit does not wait, each one's
/// outcome in the order given, the exit the first's, in that order, that
/// did not land.
fn pulls_end(
    s: &Sluice<'_>,
    group: &host_process::protocol::PullGroupWait,
    stopped: Option<i32>,
    no_wait: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let branch = group.branch.as_str();
    let state = |o: &host_process::protocol::PullItem| {
        o.last
            .as_ref()
            .map_or("waiting".to_string(), |l| l.0.clone())
    };
    if json {
        let mut items = Vec::new();
        for o in &group.items {
            let mut item = match &o.decided {
                Some(Followed::Done(sub, _)) => serde_json::to_value(sub)?,
                Some(Followed::Closed) => serde_json::json!({ "state": "closed" }),
                Some(f @ (Followed::Landed { .. } | Followed::SentBack { .. })) => across_json(f),
                _ => There::with(serde_json::json!({ "state": state(o) }), o.there.as_ref())?,
            };
            item["worktree"] = serde_json::json!(o.worktree);
            item["pull_request"] = serde_json::json!(o.number);
            if o.decided.is_none() {
                item["wait"] = serde_json::json!(pull_wait(branch, o.number));
            }
            if let (true, Some(id)) = (no_wait, o.id) {
                item["id"] = serde_json::json!(id);
                item["ahead"] = serde_json::json!(o.ahead);
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
    for o in &group.items {
        let n = o.number;
        if !json {
            writeln!(out, "#{n} from {}:", o.worktree)?;
        }
        let this = match &o.decided {
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
                // Record 0130 D4: the wait that follows it by its number.
                if !json {
                    writeln!(out, "still open: #{n}; {}", pull_wait(branch, n))?;
                }
                LANDED
            }
        };
        if code == LANDED {
            code = this;
        }
    }
    Ok(stopped.unwrap_or(code))
}

/// 137, design D25: where a pull request another host takes in stands, as
/// a wait last read it: the host that keeps its branch, as the branch's
/// records name it, and the queue sluice's check on its head carries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct There {
    taken_in_by: Option<TakenInBy>,
    queue: Option<Queue>,
}

impl There {
    /// Design D25: pull request `n` into `branch` in words: the submission
    /// the check names on the host that posted it, taken in by the host
    /// that keeps the branch; or that it waits to be taken in by that host.
    fn line(&self, n: u64, branch: &str) -> String {
        let held = self
            .queue
            .as_ref()
            .filter(|q| q.taken_in)
            .and_then(|q| Some((q.submission?, &q.host)));
        let by = self.taken_in_by.as_ref().map(|t| t.host.clone());
        let none = format!("no host keeps {branch} yet");
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

/// 152, record 0124 (design D10): the forge's refusal of the landing that
/// holds `branch`'s submission `id`, as the branch's records keep it, or,
/// once the landing has gone from them, as its `forge_refused` line logged
/// it.
fn landing_refusal(
    s: &Sluice<'_>,
    branch: &str,
    id: u64,
) -> Result<Option<sluice_core::records::LandingRefused>> {
    let snap = sluice_seams::State::read(s_state(s), branch)?;
    let log = sluice_core::line::Line::read_log(&snap)?;
    let line = sluice_core::line::Line::load(branch, snap)?;
    if let Some(r) = line.refusal_of(id) {
        return Ok(Some(r.clone()));
    }
    Ok(log.iter().rev().find_map(|l| match &l.event {
        Event::ForgeRefused { ids, words } if ids.contains(&id) => {
            Some(sluice_core::records::LandingRefused {
                words: words.clone(),
                at: l.at.clone(),
            })
        }
        _ => None,
    }))
}

fn exit_for(sub: &Submission) -> i32 {
    match sub.state {
        State::Landed => LANDED,
        State::Conflict => CONFLICT,
        State::Withdrawn => WITHDRAWN,
        State::Refused => REFUSED,
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

/// A branch's landing the forge refused, held: its submissions and the
/// forge's refusal (152; record 0124).
type HeldLanding = (Vec<u64>, sluice_core::records::LandingRefused);

/// What a row says of `s` after its columns, naming the submissions it
/// names as `names` and `said` do; `held`, the branch's landing the forge
/// refused, where the reader read it.
fn detail(
    branch: &str,
    s: &Submission,
    held: Option<&HeldLanding>,
    names: Option<&Names>,
    said: &mut BTreeSet<u64>,
) -> String {
    let shown = detail_of_state(branch, s, held, names, said);
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
    held: Option<&HeldLanding>,
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
        State::Waiting => {
            let after =
                (!s.after.is_empty()).then(|| format!("after {}", hashes(&s.after, names, said)));
            // 152, record 0124 (design D10): what it waits behind while the
            // branch's landing the forge refused is held.
            let behind = held
                .filter(|(ids, _)| !ids.contains(&s.id))
                .map(|(ids, _)| {
                    format!(
                        "waits behind the landing the forge refused of {}",
                        hashes(ids, names, said)
                    )
                });
            [after, behind]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
        }
        // 152, record 0124 (design D10): held, with the forge's words.
        State::Refused => match held {
            Some((_, refused)) => format!("held: the forge refused its landing: {}", refused.words),
            None => "held: the forge refused its landing".to_string(),
        },
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
    // Record 0122 (design D11): where its queue lives, and its move.
    if let Some(home) = &status.home {
        writeln!(out, "its queue is {}", home.line())?;
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
    // The decision `a-key-the-tree-does-not-bear-out-holds-the-branch`.
    if let Some(hold) = &status.key_held {
        writeln!(out, "{}", words::key_held(&status.branch, hold))?;
    }
    // 152, record 0124 (design D10): the landing the forge refused, held.
    if let Some((ids, refused)) = &status.landing_refused {
        writeln!(
            out,
            "{}",
            refused_landing_line(&status.branch, ids, refused)
        )?;
    }
    write_failed_releases(&status.failed_releases, out)?;
    if let Some(copy) = &status.copy {
        writeln!(out, "{}", copy_behind_line(&status.branch, copy))?;
    }
    if let Some(refused) = &status.refused {
        writeln!(out, "{}", refused_lines(refused))?;
    }
    if let Some(refused) = &status.intake_refused {
        writeln!(out, "{}", intake_refused_lines(&status.branch, refused))?;
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
            detail(
                &status.branch,
                s,
                status.landing_refused.as_ref(),
                Some(names),
                &mut said
            ),
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

fn status_json(
    s: &Sluice<'_>,
    status: &Status,
    submissions: &[Submission],
) -> Result<serde_json::Value> {
    let submissions = submissions
        .iter()
        .map(|sub| decided_document(s, sub, None))
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
        "key_held": status.key_held,
        "held_checkouts": held_checkouts,
        "copy": status.copy,
        "refused_writes": status.refused_writes,
        "home": status.home,
    });
    if let Some(refused) = &status.refused {
        json["refused"] = serde_json::json!(refused);
    }
    // 133 (design D27): this host's standing refusal of the intake.
    if let Some(refused) = &status.intake_refused {
        json["intake_refused"] = serde_json::json!(refused);
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
        crate::telemetry::say(&text);
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

/// 133 (design D27): this host's refusal of `branch`'s intake, the
/// forge's words on as many lines as it gave them.
fn intake_refused_lines(branch: &str, refused: &sluice_core::Refused) -> String {
    let rounds = match refused.rounds {
        1 => "1 round".to_string(),
        n => format!("{n} rounds"),
    };
    let mut said = refused.said.lines();
    let mut text = format!(
        "this host could not take in {branch}'s pull requests since {}, {rounds}: {}: {}",
        refused.since,
        refused.asked,
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

    /// 137 (design D25): the rows' heading names the host that keeps the
    /// branch, which takes its pull requests in, or that none keeps it
    /// yet; a row and a wait's words name that host as the one a request
    /// waits to be taken in by.
    #[test]
    fn the_rows_heading_names_the_host_that_keeps_the_branch() {
        let row = AcrossRow {
            pull_request: 41,
            title: "Add the export".into(),
            from: "export".into(),
            head: "h".into(),
            draft: false,
            state: "waiting".into(),
            queue: None,
            read_at: None,
        };
        let kept = Across {
            taken_in_by: Some(TakenInBy { host: "b".into() }),
            pull_requests: vec![row.clone()],
        };
        assert_eq!(kept.heading("main"), "pull requests b takes in:");
        assert_eq!(
            kept.line(&row),
            "#41 Add the export · waiting to be taken in by b"
        );
        let unkept = Across {
            taken_in_by: None,
            ..kept.clone()
        };
        assert_eq!(
            unkept.heading("main"),
            "pull requests this host follows; no host keeps main yet:"
        );
        assert_eq!(
            unkept.line(&row),
            "#41 Add the export · waiting to be taken in"
        );
        let there = There {
            taken_in_by: Some(TakenInBy { host: "b".into() }),
            queue: None,
        };
        assert_eq!(there.line(41, "main"), "#41 waits to be taken in by b");
        let nowhere = There {
            taken_in_by: None,
            queue: None,
        };
        assert_eq!(
            nowhere.line(41, "main"),
            "#41 waits to be taken in; no host keeps main yet"
        );
        assert_eq!(
            serde_json::to_value(&kept.taken_in_by).unwrap(),
            serde_json::json!({"host": "b"})
        );
    }
}
