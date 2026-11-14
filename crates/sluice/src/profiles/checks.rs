//! The check seam (B.2): a runner says what to run in a candidate's place
//! and reads what it reported; an executor runs it as one process group
//! under the supervisor (`crate::runs`), held to the policy's limits, and
//! says what it enforced (C.2; 36 to 38, 83, 84;
//! `docs/plans/phase-1-integrations.md` sections 3 and 4):
//!
//! - `"open"`: the process group alone; holds nothing else, confines
//!   nothing;
//! - `"sandbox"`: on macOS, Seatbelt (`sandbox-exec`) with writes allowed
//!   only to the place, its scratch directory, the host's cache
//!   directories, the store where moon keeps what it shares for the run,
//!   as the run's record names it, wherever the repository moves moon's
//!   home (C.2 sandbox row; record 0105), and what the runner names, and
//!   the network only as the policy says, the host's cache sockets
//!   whatever it says; with `[executor] docker = true`, the container
//!   engine's socket too. Not confined: a session on the host can still
//!   write the place.
//!
//! `[tests] release`, when set, is the release command (ADR 0016): a list,
//! the command first, or a string run by `sh -c`. It runs when the place
//! is released, after the run, under the same executor, toolchain and
//! environment, the place's ports included, and removes what the checks
//! handed to another process, a container engine's containers above all.
//!
//! `[tests] not_run`, a number or a list of them, names the exits by which
//! a check says it could not run here (could-not-run-1 114): a check that
//! needs something of the host brings it up and waits for it within its
//! own limit (66, 84), and exits so when it could not. sluice names no tool
//! it reads: they are the repository's.
//!
//! The policy's `[tests] runner` picks the runner, read from the branch's
//! revision with the rest of `[tests]` (56):
//!
//! - `"moon"`: the affected tasks the gate requires ([`super::moon`]);
//! - `"command"`: `sh -c <command>`, one check named by the command,
//!   never replayed.
//!
//! A run's own command, a session's or a preview's (89, 98), runs as the
//! command runner runs its command, in a working copy equipped by the
//! runner `[tests]` names (147).
//!
//! Every working copy sluice runs anything in is equipped first by one
//! sequence, [`Equipping`] (147; rule core.46): `[tests] toolchain`, then
//! the runner's own steps, each outside the executor with the run's
//! environment, stopping at the first that fails. `[tests] toolchain`,
//! when set, is the sequence's first step: a command run in the place that
//! prints shell code setting the repository's tools up (`devenv
//! print-dev-env`, `proto activate bash --export`). sluice keeps what it
//! prints and sources it before every command the runner runs, opaque to
//! it. Whatever needs the network or the user's home to set up happens in
//! the sequence, outside the executor.
//!
//! A run whose policy says `[executor] docker = true` asks the host for a
//! container engine that answers (84; record 0074): [`Tests::unmet`] looks
//! for one at the sockets the sandbox opens for such a run, by the Docker
//! Engine API's `GET /_ping`, running nothing and reading nothing of the
//! repository's (rules core.37, core.38).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::{
    log_tail, AtBound, Enforced, Equip, EquipStep, Equipped, HostRun, KeptOutTask, Limits, Network,
    NotRun, Outcome, Part, Place, Progress, Proof, Reading, Reads, Refusals, Rev, RunHandle,
    RunKind, RunReport, RunRequest, Running, Setting, Settings, Source, TaskLog, TaskProgress,
    TaskResult, Tests, Through,
};

use super::moon::{self, Moon};
use crate::host_count::{self, HostCount};
use crate::instance::{HostCache, Instance};
use crate::runs::{Ended, Request, RunState, Runs};
use crate::sessions::Sessions;

mod engine;

pub use engine::{end_containers, PLACE_LABEL};

/// What a run's place, environment and toolchain are, for a runner.
pub struct Context<'a> {
    pub request: &'a RunRequest,
    /// The command's whole environment.
    pub env: &'a BTreeMap<String, String>,
    /// What `[tests] toolchain` printed, sourced before every command.
    pub toolchain: Option<&'a Path>,
    /// The cores the run is given.
    pub jobs: usize,
}

impl Context<'_> {
    /// `argv` with the repository's toolchain set up: its script sourced,
    /// with anything it prints sent to stderr, so a command's stdout is
    /// the command's alone. The script is sourced by the `bash` on the
    /// run's PATH: what toolchains print is written for a current bash
    /// (devenv's uses bash 4's `;&`), and macOS's `/bin/bash` is 3.2.
    pub fn argv(&self, argv: &[&str]) -> Vec<String> {
        let argv = argv.iter().map(|s| s.to_string());
        match self.toolchain {
            None => argv.collect(),
            Some(script) => [
                "bash".to_string(),
                "-c".into(),
                r#"source "$0" >&2; exec "$@""#.into(),
                script.to_string_lossy().into_owned(),
            ]
            .into_iter()
            .chain(argv)
            .collect(),
        }
    }

    /// The environment a command run by [`Context::argv`] starts in: `env`
    /// as the toolchain's script leaves it once sourced, which may set
    /// variables of the runner's own (devenv's `MOON_HOME`).
    pub fn command_env(&self) -> Result<BTreeMap<String, String>> {
        if self.toolchain.is_none() {
            return Ok(self.env.clone());
        }
        let argv = self.argv(&["env", "-0"]);
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&self.request.place.path)
            .env_clear()
            .envs(self.env)
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("running {}", argv.join(" ")))?;
        if !out.status.success() {
            bail!(
                "{} exited {}: {}",
                argv.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out
            .stdout
            .split(|b| *b == 0)
            .filter_map(|entry| {
                let entry = String::from_utf8_lossy(entry);
                let (k, v) = entry.split_once('=')?;
                Some((k.to_string(), v.to_string()))
            })
            .collect())
    }
}

/// What a runner decided to run.
pub struct Plan {
    /// The command, or `None` when the candidate affects no required
    /// check: a pass with nothing run, and no tool called.
    pub argv: Option<Vec<String>>,
    /// The checks the run is to give a result for.
    pub targets: Vec<String>,
    /// Whether the runner holds the run to `jobs` cores itself.
    pub holds_processors: bool,
    /// Names of files the runner's tools stamp wherever they are installed
    /// (proto's `.last-used`), which a sandbox lets the run write.
    pub stamps: Vec<&'static str>,
    /// Targets the runner keeps out of its CI mode, so never runs (moon's
    /// `runInCI: false`); each reads missing.
    pub kept_out: Vec<String>,
    /// Whether the run holds a test `[tests] cold` names, so is given none
    /// of the host's caches (35).
    pub cold: bool,
    /// 149: each target's dependencies at any depth of the runner's graph,
    /// read as the run is planned, since the runner's report names no edge
    /// of it; empty for the command runner, which runs one command.
    pub deps: BTreeMap<String, Vec<String>>,
}

/// What a run's request carries for reading its result.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Meta {
    runner: String,
    revision: String,
    place: PathBuf,
    targets: Vec<String>,
    executor: Enforced,
    host: String,
    /// When the run started, in milliseconds since the epoch: a task's run
    /// record from before it is no end of this run's (ADR 0028).
    #[serde(default)]
    started_ms: Option<i64>,
    /// `[tests] not_run`: exits by which a check says it could not run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    not_run: Vec<i32>,
    /// `Plan::kept_out`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    kept_out: Vec<String>,
    /// `Plan::deps`; a run an older sluice started has none, and reads no
    /// target through a dependency (149).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    deps: BTreeMap<String, Vec<String>>,
    /// Where moon keeps the run's manifests, found as the run started
    /// (design D8); a run recorded without it keeps them in its place's
    /// `.moon/cache`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    store: Option<PathBuf>,
    /// 84, record 0097 (design D1, D2): the store path the active
    /// system's link named as the run began, before its checkout was
    /// equipped; none on a host Nix doesn't manage, and on a run an older
    /// sluice started, each read as before (D4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nix_system: Option<PathBuf>,
    /// 139, design D4: the mark each refusal rule of a sandboxed run's
    /// profile carries; none for an open run and for a run an older sluice
    /// started, whose refusals are never read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mark: Option<String>,
}

/// A pool the binding hands this profile, to start runs on (88; ADR 0034).
pub trait Launcher {
    fn launch(&self, launch: &sluice_seams::Launch) -> Result<()>;
    fn job_alive(
        &self,
        kind: &(String, Setting),
        claim: &str,
        remote: Option<&str>,
    ) -> Result<bool>;
    fn wake(&self, kind: &(String, Setting), branch: &str, remote: Option<&str>) -> Result<()>;
}

/// The check seam's profile, for the repository whose common dir is
/// `common`.
pub struct Checks {
    /// The repository's common git directory.
    common: PathBuf,
    runs: Runs,
    instance: Instance,
    /// The pool this host can start runs on, where the binding gave one.
    pool: Option<Box<dyn Launcher>>,
    /// The host's session runs, when not the host's own directory.
    sessions: Option<Sessions>,
    /// The host's running checks, when not the host's own directory.
    running: Option<HostCount>,
    /// Where the container engine is looked for, when not where the
    /// sandbox lets a run reach it.
    engines: Option<Vec<Endpoint>>,
    /// The link the active system is read from, when not the host's.
    system_link: Option<PathBuf>,
    /// The program a sandboxed run's reports are streamed with, when not
    /// the host's.
    log: Option<PathBuf>,
}

impl Checks {
    /// `program` is the `sluice` executable that supervises a run.
    pub fn new(common: &Path, program: PathBuf, instance: Instance) -> Self {
        Checks {
            common: common.to_path_buf(),
            runs: Runs::new(common, program),
            instance,
            sessions: None,
            running: None,
            pool: None,
            engines: None,
            system_link: None,
            log: None,
        }
    }

    /// The same profile, starting runs on `pool` (88).
    pub fn with_pool(mut self, pool: Box<dyn Launcher>) -> Self {
        self.pool = Some(pool);
        self
    }

    /// The same profile, keeping the host's session runs under `dir`.
    pub fn with_sessions(mut self, dir: PathBuf) -> Self {
        self.sessions = Some(Sessions::new(dir));
        self
    }

    /// The same profile, counting the host's running checks under `dir`.
    pub fn with_running(mut self, dir: PathBuf) -> Self {
        self.running = Some(HostCount::new(dir));
        self
    }

    /// The same profile, looking for the container engine at the unix
    /// sockets `sockets` in place of where the sandbox lets a run reach it
    /// (design D2).
    pub fn with_engine_sockets(self, sockets: Vec<PathBuf>) -> Self {
        self.with_engine_endpoints(sockets.into_iter().map(Endpoint::Unix).collect())
    }

    /// The same profile, looking for the container engine at `endpoints`,
    /// and letting a sandboxed run reach it there, in place of the host's
    /// (design D5).
    pub fn with_engine_endpoints(mut self, endpoints: Vec<Endpoint>) -> Self {
        self.engines = Some(endpoints);
        self
    }

    /// The same profile, reading the active system from the link at
    /// `path` in place of the host's (design D8).
    pub fn with_system_link(mut self, path: PathBuf) -> Self {
        self.system_link = Some(path);
        self
    }

    /// The same profile, streaming a sandboxed run's reports with the
    /// program at `path` in place of the host's `log` (design D5): how a
    /// test stands for a log that cannot be streamed.
    pub fn with_log(mut self, path: PathBuf) -> Self {
        self.log = Some(path);
        self
    }

    /// The link the active system is read from: the one this profile was
    /// given, else the one [`SYSTEM_LINK_VAR`] names, else the host's
    /// [`SYSTEM_LINK`] (design D1, D8).
    fn system_link(&self) -> PathBuf {
        match &self.system_link {
            Some(p) => p.clone(),
            None => std::env::var_os(SYSTEM_LINK_VAR)
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from(SYSTEM_LINK), PathBuf::from),
        }
    }

    /// Where the container engine is looked for: the endpoints this
    /// profile was given, else those the sandbox lets a run that may reach
    /// the engine reach (design D2, D5).
    fn engines(&self) -> Vec<Endpoint> {
        match &self.engines {
            Some(e) => e.clone(),
            None => engine_endpoints(),
        }
    }

    /// The host's session runs, with what each `sluice exec` keeps of its
    /// run (session-runs-1 clauses 89, 91; ADR 0035).
    pub fn host_sessions(&self) -> Result<Sessions> {
        match &self.sessions {
            Some(s) => Ok(s.clone()),
            None => Sessions::host(),
        }
    }

    /// The host's running checks, every repository's: the count every run
    /// is admitted against (71).
    pub fn host_count(&self) -> Result<HostCount> {
        match &self.running {
            Some(c) => Ok(c.clone()),
            None => HostCount::host(),
        }
    }

    pub fn runs(&self) -> &Runs {
        &self.runs
    }

    /// The host's settings the profile was bound with.
    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    /// 71 (rule core.49; record 0087): a part of `kind` through the
    /// host's registry: a combination's under the bound, a session's or a
    /// preview's under the bound less the reserve (91, 102), admitted with
    /// no run directory yet, which its start names. `None` at the bound.
    fn admit(&self, kind: RunKind) -> Result<Option<Part>> {
        let (entry_kind, limit) = match kind {
            RunKind::Queue => (host_count::Kind::Queue, self.instance.bound),
            RunKind::Session => (
                host_count::Kind::Session,
                self.instance.bound.saturating_sub(self.instance.reserved),
            ),
        };
        let entry = host_count::Entry {
            kind: entry_kind,
            repository: self.common.clone(),
            place: String::new(),
            run_dir: PathBuf::new(),
            pid: std::process::id(),
            admitted: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        Ok(self
            .host_count()?
            .admit(&entry, limit)?
            .map(|a| Part::new(kind, Holding(a))))
    }
}

/// A part of the host's bound held through its registry (71; record
/// 0087): its entry names its run directory once the run begins, and its
/// hold keeps it counted until the run's supervisor holds that directory.
/// Dropped unstarted, its entry goes.
struct Holding(host_count::Admission);

/// The executors this host has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Open,
    Sandbox,
}

/// 84: the executor the policy names (`open` when it names none), or a
/// refusal naming the limit it cannot hold, before anything runs.
fn choose(limits: &Limits) -> Result<Kind> {
    let kind = match limits.executor.as_deref() {
        None | Some("open") => Kind::Open,
        Some("sandbox") => Kind::Sandbox,
        Some("container") => {
            bail!("[executor] kind = \"container\": the container executor is not built yet")
        }
        Some(k) => {
            bail!("[executor] kind = {k:?} names no executor; one of open, sandbox, container")
        }
    };
    match kind {
        Kind::Open => {
            if let Some(m) = limits.memory {
                bail!(
                    "[executor] memory = {m} bytes: the open executor cannot hold a memory limit"
                );
            }
            if let Some(n @ (Network::Denied | Network::Localhost)) = limits.network {
                bail!("[executor] network = {n:?}: the open executor cannot hold a run's network");
            }
        }
        Kind::Sandbox => {
            if !cfg!(target_os = "macos") {
                bail!("[executor] kind = \"sandbox\": on this host the sandbox executor is not built yet (bubblewrap and Landlock)");
            }
            if let Some(m) = limits.memory {
                bail!("[executor] memory = {m} bytes: the sandbox executor on macOS cannot hold a memory limit");
            }
        }
    }
    Ok(kind)
}

/// Design D1: what a path is to a sandboxed run's profile. The grant
/// digests the profile with each path named by its role, so places and
/// hosts given one profile give one grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    /// The place's checkout.
    Place,
    /// The place's scratch directory.
    Scratch,
    /// A directory every place's runs share: a host cache's, or the
    /// runner's store outside the place (record 0105).
    Cache,
    /// A host cache's socket.
    CacheSocket,
    /// The container engine's unix socket (ADR 0016).
    EngineSocket,
    /// The container engine's TCP address (design D5 of its change).
    EngineTcp,
    /// The user's own temporary directory.
    UserTmp,
}

impl Role {
    fn token(self) -> &'static str {
        match self {
            Role::Place => "<place>",
            Role::Scratch => "<scratch>",
            Role::Cache => "<cache>",
            Role::CacheSocket => "<cache-socket>",
            Role::EngineSocket => "<engine-socket>",
            Role::EngineTcp => "<engine-tcp>",
            Role::UserTmp => "<user-tmp>",
        }
    }
}

/// Design D1: what a sandboxed run may reach, each path with its role, as
/// [`seatbelt`] writes its profile from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Reach {
    /// What it may write: the place's checkout and scratch, then the
    /// shared directories, each by its role.
    writable: Vec<(Role, PathBuf)>,
    /// Names of files the runner's tools stamp wherever they are installed.
    stamps: Vec<String>,
    network: Option<Network>,
    /// The unix sockets it may reach whatever the network: the engine's,
    /// then the caches'.
    sockets: Vec<(Role, PathBuf)>,
    /// The container engine's TCP addresses, by host and port.
    tcp: Vec<(String, u16)>,
    /// The user's own temporary directory, resolved.
    user_tmp: Option<PathBuf>,
    /// The run's own mark, which each refusal rule carries so the kernel's
    /// report of a refusal names the run (139; design D4); `None` for a
    /// profile no run's refusals are read from.
    mark: Option<String>,
}

/// A sandboxed run's profile, and the grant it gives (83, 138; design D1).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Profile {
    /// The Seatbelt profile the run is held to.
    text: String,
    /// `sb1:` and the first 16 bytes of the SHA-256 of the profile with
    /// every path named by its role, in hex.
    grant: String,
}

impl Reach {
    /// The profile written for the run, and its grant.
    fn profile(&self) -> Profile {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(seatbelt(self, true).as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        Profile {
            text: seatbelt(self, false),
            grant: format!("sb1:{hex}"),
        }
    }
}

/// A Seatbelt profile: everything a user may do, but writes only to
/// `reach.writable`, files named in its stamps, and the terminal and null
/// devices, a new pseudo-terminal's included;
/// the network as `reach.network` says: `Denied`, none, local sockets
/// included; `Localhost`, listening and connecting on localhost only, by
/// address or by name, and no other connection (name lookups included);
/// and, whatever the network, each of its sockets and the container
/// engine's TCP addresses, by host and port (ADR 0016; design D5).
///
/// Design D4: each refusal rule carries the run's mark, `(with message
/// "<mark>")`, so the kernel's report of what it refused names the run;
/// after the allowed writes, a write to `/dev/dtracehelper`, which every
/// program's loader is refused and goes on from and no check needs, is
/// refused with no report.
///
/// Design D1: with `roles`, each path is named by its role and each line
/// is kept once, in order, so two hosts with two and three caches, or a
/// path given and resolved, render the same; this is the text the grant
/// digests. Stamps, literal device paths and the resolver's socket are
/// written as they are either way, so the profile written and its grant
/// cannot drift.
fn seatbelt(reach: &Reach, roles: bool) -> String {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let path = |role: Role, p: &Path| match roles {
        true => role.token().to_string(),
        false => quote(&p.to_string_lossy()),
    };
    // Where Chromium keeps the socket that makes it one instance per
    // profile: the user's own temporary directory, whatever `TMPDIR` says,
    // so a browser test can start one.
    let browser = reach.user_tmp.as_ref().map(|t| {
        let t = match roles {
            true => Role::UserTmp.token().to_string(),
            false => regex_quote(&t.to_string_lossy()),
        };
        format!(
            "(regex #\"^{t}/\\.?(com\\.google\\.[Cc]hrome|org\\.chromium\\.[Cc]hromium)[.a-zA-Z]*\\.[A-Za-z0-9]+(/|$)\")"
        )
    });
    // Design D4: the run's mark on each refusal rule, `<mark>` by role.
    let deny = |what: &str| match (&reach.mark, roles) {
        (Some(_), true) => format!("(deny {what} (with message \"<mark>\"))\n"),
        (Some(m), false) => format!("(deny {what} (with message \"{}\"))\n", quote(m)),
        (None, _) => format!("(deny {what})\n"),
    };
    let mut p = String::from("(version 1)\n(allow default)\n");
    p.push_str(&deny("file-write*"));
    p.push_str("(allow file-write*\n");
    for (role, w) in &reach.writable {
        p.push_str(&format!("  (subpath \"{}\")\n", path(*role, w)));
    }
    if let Some(b) = &browser {
        p.push_str(&format!("  {b}\n"));
    }
    for s in &reach.stamps {
        p.push_str(&format!("  (regex #\"/{}$\")\n", regex_escape(s)));
    }
    // C.3: the null and terminal devices, and `/dev/ptmx`, from which a
    // command opens a pseudo-terminal of its own, whose end is a `ttys`.
    p.push_str("  (literal \"/dev/null\") (literal \"/dev/tty\") (literal \"/dev/ptmx\") (regex #\"^/dev/fd/\") (regex #\"^/dev/ttys\"))\n");
    // Design D4: refused as before, and reported by no one.
    p.push_str("(deny file-write-data (literal \"/dev/dtracehelper\") (with no-report))\n");
    match reach.network {
        Some(Network::Denied) => p.push_str(&deny("network*")),
        Some(Network::Localhost) => {
            p.push_str(&deny("network*"));
            p.push_str(
                "(allow network-bind network-inbound (local ip \"localhost:*\"))\n\
                 (allow network-outbound (remote ip \"localhost:*\"))\n",
            );
        }
        Some(Network::Allowed) | None => {}
    }
    // A local socket of the run's own, its browser's included, unless the
    // run may have no connection at all.
    if reach.network != Some(Network::Denied) {
        let mut own: Vec<String> = reach
            .writable
            .iter()
            .map(|(role, w)| format!("(subpath \"{}\")", path(*role, w)))
            .collect();
        own.extend(browser);
        // One rule a path: a socket filter takes one path, not a list.
        for o in own {
            p.push_str(&format!(
                "(allow network-bind network-inbound network-outbound (local unix-socket {o}))\n\
                 (allow network-outbound (remote unix-socket {o}))\n"
            ));
        }
    }
    // ADR 0016: the container engine's socket, whatever the network; and
    // 35, design D4: each cache's.
    for (role, s) in &reach.sockets {
        p.push_str(&format!(
            "(allow network-outbound (remote unix-socket (path-literal \"{}\")))\n",
            path(*role, s)
        ));
    }
    // Design D5: the engine's TCP address, whatever the network. Seatbelt
    // names a remote host only as `localhost` or `*`; a name is resolved
    // through mDNSResponder's socket.
    for (host, port) in &reach.tcp {
        let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1");
        let at = if local { "localhost" } else { "*" };
        let address = match roles {
            true => Role::EngineTcp.token().to_string(),
            false => format!("{at}:{port}"),
        };
        p.push_str(&format!(
            "(allow network-outbound (remote tcp \"{address}\"))\n"
        ));
        if host.parse::<std::net::IpAddr>().is_err() && !local {
            p.push_str(
                "(allow network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))\n",
            );
        }
    }
    if !roles {
        return p;
    }
    let mut seen = std::collections::HashSet::new();
    p.lines()
        .filter(|l| seen.insert(*l))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// A path as a Seatbelt regular expression matching it literally.
fn regex_quote(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' {
                vec![c]
            } else {
                vec!['\\', c]
            }
        })
        .collect()
}

/// The user's own temporary directory on macOS, resolved, which some
/// tools use whatever `TMPDIR` says.
fn user_tmp() -> Option<PathBuf> {
    let out = std::process::Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .ok()?;
    let dir = String::from_utf8(out.stdout).ok()?;
    std::fs::canonicalize(dir.trim()).ok()
}

fn regex_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                vec![c]
            } else {
                vec!['\\', c]
            }
        })
        .collect()
}

/// `[tests] cold`: the tests a run holding any of is given no cache, named
/// as the gate's `required` names them, a target or a `#tag` (35).
pub fn cold_tests(tests: &Settings) -> Result<Vec<String>> {
    match tests.get("cold") {
        None => Ok(Vec::new()),
        Some(Setting::List(xs)) => Ok(xs.clone()),
        Some(_) => bail!("[tests] cold must be a list of tests, named as required names them"),
    }
}

fn text<'a>(settings: &'a Settings, key: &str) -> Result<Option<&'a str>> {
    match settings.get(key) {
        None => Ok(None),
        Some(Setting::Text(s)) => Ok(Some(s)),
        Some(_) => bail!("[tests] {key} must be a string"),
    }
}

/// `[tests] toolchain`: the command, or `None`.
fn toolchain_argv(tests: &Settings) -> Result<Option<Vec<String>>> {
    match tests.get("toolchain") {
        None => Ok(None),
        Some(Setting::List(xs)) if !xs.is_empty() => Ok(Some(xs.clone())),
        Some(_) => bail!("[tests] toolchain must be a list of strings, the command first"),
    }
}

/// The toolchain step's name.
const TOOLCHAIN: &str = "toolchain";

/// 147: the toolchain step: `[tests] toolchain` run in the place, outside
/// the executor, and what it prints kept in `script`. Its words are its
/// error stream alone, since its output is the script.
fn toolchain(
    argv: &[String],
    place: &Path,
    env: &BTreeMap<String, String>,
    script: &Path,
) -> Result<EquipStep> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("[tests] toolchain names no command"))?;
    let out = match std::process::Command::new(program)
        .args(args)
        .current_dir(place)
        .env_clear()
        .envs(env)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) => out,
        Err(e) => return Ok(failed(TOOLCHAIN, &format!("could not be run: {e}"), "")),
    };
    let said = printed(&[&out.stderr]);
    match why(out.status) {
        Some(reason) => Ok(failed(TOOLCHAIN, &reason, &said)),
        None => {
            std::fs::write(script, out.stdout)
                .with_context(|| format!("writing {}", script.display()))?;
            Ok(passed(TOOLCHAIN, said))
        }
    }
}

/// 147: what an equip step printed, each of `parts` as text and trimmed,
/// one to a line, the empty ones left out.
pub fn printed(parts: &[&[u8]]) -> String {
    parts
        .iter()
        .map(|p| String::from_utf8_lossy(p).trim().to_string())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 147: why a step's command did not pass, `exited <code>` or `ended by
/// signal <n>`; `None` where it exited 0.
pub fn why(status: std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt as _;
    if status.success() {
        return None;
    }
    Some(match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited {code}"),
        (None, Some(signal)) => format!("ended by signal {signal}"),
        (None, None) => format!("ended {status}"),
    })
}

/// 147: the step `name`, passed, with what it printed.
pub fn passed(name: &str, printed: String) -> EquipStep {
    EquipStep {
        step: name.into(),
        passed: true,
        words: printed,
    }
}

/// 147: the step `name`, failed for `reason`, with what it printed after
/// it where it printed anything.
pub fn failed(name: &str, reason: &str, printed: &str) -> EquipStep {
    EquipStep {
        step: name.into(),
        passed: false,
        words: match printed {
            "" => reason.into(),
            said => format!("{reason}: {said}"),
        },
    }
}

/// 147; rule core.46: the one sequence that equips a working copy before
/// sluice runs anything there, the request's place: the toolchain
/// `[tests]` names, then the steps of the runner it names, each with the
/// stop it belongs to, run outside the executor with `env`. Asked through
/// [`Through::Reading`] and later through [`Through::Running`], it runs
/// only the steps it had not run, so a run's equipping is one sequence
/// whose middle is the runner's queries. No step runs after one that
/// failed.
pub struct Equipping<'a> {
    request: &'a RunRequest,
    env: &'a BTreeMap<String, String>,
    jobs: usize,
    /// `[tests] toolchain`, where set.
    toolchain: Option<Vec<String>>,
    /// Where what the toolchain prints is kept.
    script: &'a Path,
    /// The runner's steps, in order.
    runner: Vec<moon::Step>,
    /// The steps run so far, in order.
    steps: Vec<EquipStep>,
}

impl<'a> Equipping<'a> {
    /// The sequence for `request`'s place by its `[tests]`, each step run
    /// with `env`, the toolchain's output kept in `script`. Runs nothing.
    pub fn new(
        request: &'a RunRequest,
        env: &'a BTreeMap<String, String>,
        jobs: usize,
        script: &'a Path,
    ) -> Result<Self> {
        let toolchain = toolchain_argv(&request.tests)?;
        let runner = match text(&request.tests, "runner")? {
            Some("moon") => Moon.steps(&request.place.path),
            Some("command") => Vec::new(),
            Some(other) => {
                bail!("[tests] runner = {other:?} names no runner: \"moon\" or \"command\"")
            }
            None => bail!("[tests] names no runner: \"moon\" or \"command\""),
        };
        Ok(Equipping {
            request,
            env,
            jobs,
            toolchain,
            script,
            runner,
            steps: Vec::new(),
        })
    }

    /// Run each step not yet run up to `stop`, in order, stopping at the
    /// first that fails. `Err` is sluice's own failure, never a step's.
    pub fn through(&mut self, stop: Through) -> Result<()> {
        let first = usize::from(self.toolchain.is_some());
        loop {
            if self.failed().is_some() {
                return Ok(());
            }
            let next = self.steps.len();
            let step = match (&self.toolchain, next.checked_sub(first)) {
                (Some(argv), None) => {
                    toolchain(argv, &self.request.place.path, self.env, self.script)?
                }
                (_, Some(i)) => match self.runner.get(i) {
                    Some(step) if step.through <= stop => (step.run)(&Moon, &self.context()),
                    _ => return Ok(()),
                },
                (None, None) => unreachable!("with no toolchain the runner's steps are first"),
            };
            self.steps.push(step);
        }
    }

    /// [`Equipping::through`], failing with the step's name and its words
    /// where a step failed: a run whose working copy could not be equipped
    /// could not start (114).
    pub fn ready(&mut self, stop: Through) -> Result<()> {
        self.through(stop)?;
        match self.failed() {
            Some(step) => bail!("{}: {}", step.step, step.words),
            None => Ok(()),
        }
    }

    /// The step that failed, where one did.
    pub fn failed(&self) -> Option<&EquipStep> {
        self.steps.iter().find(|s| !s.passed)
    }

    /// The `Context` the runner's queries, plan and run use: the place and
    /// environment the steps had, with the toolchain's script sourced
    /// where the toolchain ran.
    pub fn context(&self) -> Context<'a> {
        let sourced = self.toolchain.is_some()
            && self
                .steps
                .first()
                .is_some_and(|s| s.step == TOOLCHAIN && s.passed);
        Context {
            request: self.request,
            env: self.env,
            toolchain: sourced.then_some(self.script),
            jobs: self.jobs,
        }
    }

    /// The steps run, in order.
    pub fn equipped(self) -> Equipped {
        Equipped { steps: self.steps }
    }
}

/// What a check may need of sluice's own environment: paths, the user,
/// the locale, the terminal, and the tool homes `PATH` points into.
const KEEP: [&str; 10] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "TERM",
    // A proto shim on PATH resolves its tool under its home.
    "PROTO_HOME",
    "MOON_HOME",
];

/// Whether sluice sets `name` for every run itself: what it keeps of its
/// own environment, the locale's `LC_*` and its own `SLUICE_*`. A host's
/// cache declares none of them, so none is silently replaced (35).
pub fn sets_itself(name: &str) -> bool {
    KEEP.contains(&name) || name.starts_with("LC_") || name.starts_with("SLUICE_")
}

/// The command's environment: what a check may need of sluice's own
/// (`KEEP`), the place's paths, the variables the host's `caches` declare,
/// and nothing else that names another place or tool (35): no `GIT_*`,
/// other `MOON_*`, `DEVENV_*` or `DIRENV_*`.
fn environment(
    request: &RunRequest,
    jobs: usize,
    caches: &[HostCache],
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| KEEP.contains(&k.as_str()) || k.starts_with("LC_"))
        .collect();
    env.insert(
        "SLUICE_CHECKOUT".into(),
        request.place.path.to_string_lossy().into(),
    );
    env.insert(
        "SLUICE_TMP".into(),
        request.place.tmp.to_string_lossy().into(),
    );
    env.insert("SLUICE_JOBS".into(), jobs.to_string());
    // 31: the place's short name and its ports.
    env.extend(request.place.env.clone());
    // 35: the host's caches, as its config declares them; none of them is
    // a variable sluice sets itself (`sets_itself`).
    for cache in caches {
        env.extend(cache.env.clone());
    }
    // The container engine's client finds the engine as the user's does,
    // over TLS where the user's would (design D5).
    if request.limits.docker {
        for k in [
            "DOCKER_HOST",
            "DOCKER_CONTEXT",
            "DOCKER_CONFIG",
            "DOCKER_TLS_VERIFY",
            "DOCKER_TLS",
            "DOCKER_CERT_PATH",
        ] {
            if let Ok(v) = std::env::var(k) {
                env.insert(k.into(), v);
            }
        }
    }
    env
}

/// 35: a cache whose `dir` is the run's checkout, holds it or lies inside
/// it would let one checkout write another's; the run is refused before
/// anything of it runs (design D2).
fn outside_checkout(checkout: &Path, caches: &[HostCache]) -> Result<()> {
    let checkout = resolved(checkout);
    for cache in caches {
        let dir = resolved(&cache.dir);
        let how = if dir == checkout {
            "is"
        } else if checkout.starts_with(&dir) {
            "holds"
        } else if dir.starts_with(&checkout) {
            "lies inside"
        } else {
            continue;
        };
        bail!(
            "the host's [[caches]] dir {} {how} this run's checkout {}: a shared cache lies \
             outside every checkout",
            cache.dir.display(),
            checkout.display()
        );
    }
    Ok(())
}

/// `path` with its longest part that exists resolved, the rest as given.
fn resolved(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |p, c| p.join(c));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                at = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Design D5: where a run with `docker = true` reaches the container
/// engine, and where the engine is looked for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Endpoint {
    /// A unix socket, by its path.
    Unix(PathBuf),
    /// A TCP address, over TLS where `tls` says.
    Tcp {
        host: String,
        port: u16,
        tls: Option<EngineTls>,
    },
}

/// How the engine's client speaks TLS to it, as `DOCKER_TLS_VERIFY`,
/// `DOCKER_TLS` and `DOCKER_CERT_PATH` say (design D5).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EngineTls {
    /// The engine's certificate is verified against `ca.pem`.
    pub verify: bool,
    /// Where `ca.pem`, `cert.pem` and `key.pem` are.
    pub certs: PathBuf,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endpoint::Unix(path) => write!(f, "{}", path.display()),
            Endpoint::Tcp { port, .. } => write!(f, "tcp://{}:{port}", self.host_part()),
        }
    }
}

impl Endpoint {
    /// A TCP endpoint's host as an address names it: an IPv6 address in
    /// brackets.
    fn host_part(&self) -> String {
        match self {
            Endpoint::Tcp { host, .. } if host.contains(':') => format!("[{host}]"),
            Endpoint::Tcp { host, .. } => host.clone(),
            Endpoint::Unix(_) => String::new(),
        }
    }
}

/// The container engine's endpoints a run with `docker = true` may reach,
/// and the ones the engine is looked for at (design D2, D5): those
/// [`ENGINE_SOCKETS_VAR`] names where it is set, else `DOCKER_HOST`'s, a
/// unix socket or a TCP address, and the usual places engines on macOS
/// put their sockets; each socket by the path given and the path it
/// resolves to.
pub fn engine_endpoints() -> Vec<Endpoint> {
    let tls = engine_tls();
    let mut found = Vec::new();
    if let Some(v) = std::env::var_os(ENGINE_SOCKETS_VAR) {
        for e in listed(&v.to_string_lossy()) {
            match e.starts_with("tcp://") {
                true => found.extend(docker_host(e, tls.clone())),
                false => found.push(Endpoint::Unix(e.into())),
            }
        }
    } else {
        if let Some(e) = std::env::var("DOCKER_HOST")
            .ok()
            .and_then(|h| docker_host(&h, tls))
        {
            found.push(e);
        }
        found.push(Endpoint::Unix("/var/run/docker.sock".into()));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for p in [
                ".docker/run/docker.sock",
                ".orbstack/run/docker.sock",
                ".colima/default/docker.sock",
            ] {
                found.push(Endpoint::Unix(home.join(p)));
            }
        }
    }
    let mut all = Vec::new();
    for e in found {
        if let Endpoint::Unix(p) = &e {
            if let Ok(real) = std::fs::canonicalize(p) {
                all.push(Endpoint::Unix(real));
            }
        }
        all.push(e);
    }
    all.sort();
    all.dedup();
    all
}

/// The entries of [`ENGINE_SOCKETS_VAR`]: separated by `:` where the next
/// begins with `/` or `tcp://`, so an address's own colons stay in it.
fn listed(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    for (at, _) in v.match_indices(':') {
        let next = &v[at + 1..];
        let starts =
            next.starts_with("tcp://") || (next.starts_with('/') && !next.starts_with("//"));
        if starts && at > from {
            out.push(&v[from..at]);
            from = at + 1;
        }
    }
    if from < v.len() {
        out.push(&v[from..]);
    }
    out
}

/// How the engine's client speaks TLS where it is reached by TCP:
/// verifying the engine under `DOCKER_TLS_VERIFY`, not under `DOCKER_TLS`
/// alone, with the files of `DOCKER_CERT_PATH`, else of `~/.docker`; `None`
/// where neither is set (design D5).
fn engine_tls() -> Option<EngineTls> {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let verify = set("DOCKER_TLS_VERIFY");
    if !verify && !set("DOCKER_TLS") {
        return None;
    }
    let certs = match std::env::var_os("DOCKER_CERT_PATH").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".docker"),
    };
    Some(EngineTls { verify, certs })
}

/// The endpoint `DOCKER_HOST` names, as the engine's client reads it:
/// `unix://<path>`, or `tcp://<host>[:<port>]`, the port 2376 over TLS and
/// 2375 without where none is given; `None` for any other scheme
/// (`ssh://`, `npipe://`), which the look does not read (design D5).
pub fn docker_host(value: &str, tls: Option<EngineTls>) -> Option<Endpoint> {
    if let Some(path) = value.strip_prefix("unix://") {
        return Some(Endpoint::Unix(path.into()));
    }
    let rest = value.strip_prefix("tcp://")?.trim_end_matches('/');
    let (host, port) = match rest.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            (host, after.strip_prefix(':'))
        }
        None => match rest.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        },
    };
    let port = match port {
        Some(p) => p.parse().ok()?,
        None if tls.is_some() => 2376,
        None => 2375,
    };
    (!host.is_empty()).then(|| Endpoint::Tcp {
        host: host.into(),
        port,
        tls,
    })
}

/// The tests' way to name where a fake container engine listens, as a
/// list of unix sockets' paths and `tcp://` addresses: where set, these
/// replace the host's endpoints both where the engine is looked for and
/// where the sandbox lets a run reach it, so the look finds only what the
/// run would reach (design D2, D5). A host names its engine by
/// `DOCKER_HOST`.
pub const ENGINE_SOCKETS_VAR: &str = "SLUICE_ENGINE_SOCKETS";

/// The link NixOS and nix-darwin keep to the active system's store path,
/// pointed at another by every switch (84, record 0097; design D1). A host
/// where it is no link is one Nix doesn't manage, and nothing of its
/// system is read.
pub const SYSTEM_LINK: &str = "/run/current-system";

/// The tests' way to name the link the active system is read from, in
/// place of [`SYSTEM_LINK`], as [`ENGINE_SOCKETS_VAR`] names the engine's
/// sockets (design D8).
pub const SYSTEM_LINK_VAR: &str = "SLUICE_SYSTEM_LINK";

/// 84, record 0097 (design D1): the store path `link` names, read as a
/// link and nothing more, no command run (rule core.37); `None` where it
/// is no link, or cannot be read.
fn nix_system(link: &Path) -> Option<PathBuf> {
    std::fs::read_link(link).ok()
}

/// 84, 114, record 0097 (design D4, D5): the words of a switch, where the
/// run noted a system as it `began` and the link names another `now`;
/// `None` where it noted none, the link names none now, or both name one.
fn switch(began: Option<&Path>, now: Option<&Path>) -> Option<String> {
    let (from, to) = (began?, now?);
    (from != to).then(|| {
        format!(
            "the host's system was switched during the run, from {} to {}",
            from.display(),
            to.display()
        )
    })
}

/// How long the engine look waits for an endpoint to take its connection,
/// and again for its answer (design D2).
const ENGINE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// 84, record 0074 (design D2, D5): `None` when a container engine answers
/// at one of `endpoints`, by the Docker Engine API's `GET /_ping` over each
/// unix socket that exists and each TCP address, the first answering `200`
/// ending the look; otherwise why not, beginning with what is missing
/// (design D3): `no container engine answers: `, then each endpoint tried
/// with what it said, or, where none was, the paths looked at. It runs as
/// the process that would run the checks, so it finds what their run would
/// reach.
pub fn engine_unmet(endpoints: &[Endpoint]) -> Option<String> {
    let mut said = Vec::new();
    let mut looked = std::collections::BTreeSet::new();
    for e in endpoints {
        let answer = match e {
            Endpoint::Unix(socket) => {
                if !socket.exists() {
                    continue;
                }
                // A path and the path it resolves to are one socket.
                let real = std::fs::canonicalize(socket).unwrap_or_else(|_| socket.clone());
                if !looked.insert(real) {
                    continue;
                }
                ping(socket)
            }
            Endpoint::Tcp { port, tls, .. } => ping_tcp(&e.host_part(), *port, tls.as_ref()),
        };
        match answer {
            Ok(()) => return None,
            Err(why) => said.push(format!("{e} {why}")),
        }
    }
    let said = if said.is_empty() {
        let paths: Vec<String> = endpoints.iter().map(|e| e.to_string()).collect();
        match paths.is_empty() {
            true => "no socket to look at".into(),
            false => format!("no socket at {}", paths.join(", ")),
        }
    } else {
        said.join("; ")
    };
    Some(format!("no container engine answers: {said}"))
}

/// `GET /_ping` over the unix socket `socket`: `Ok` when it answers `200`,
/// else what it did, in words. The connection is made on a thread of its
/// own, so a socket that never takes it holds the look no longer than
/// [`ENGINE_WAIT`].
fn ping(socket: &Path) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};
    enum Step {
        Connected,
        Said(std::io::Result<String>),
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let path = socket.to_path_buf();
    std::thread::spawn(move || {
        let said = (|| {
            let mut s = std::os::unix::net::UnixStream::connect(&path)?;
            let _ = tx.send(Step::Connected);
            s.set_read_timeout(Some(ENGINE_WAIT))?;
            s.set_write_timeout(Some(ENGINE_WAIT))?;
            s.write_all(b"GET /_ping HTTP/1.0\r\nHost: docker\r\n\r\n")?;
            let mut status = String::new();
            BufReader::new(s.take(512)).read_line(&mut status)?;
            Ok(status)
        })();
        let _ = tx.send(Step::Said(said));
    });
    let wait = format!("{}s", ENGINE_WAIT.as_secs());
    let said = match rx.recv_timeout(ENGINE_WAIT) {
        Ok(Step::Connected) => match rx.recv_timeout(ENGINE_WAIT * 2) {
            Ok(Step::Said(said)) => said,
            _ => return Err(format!("gave no answer within {wait}")),
        },
        Ok(Step::Said(said)) => said,
        Err(_) => return Err(format!("took no connection within {wait}")),
    };
    match said {
        Ok(status) => match status.split_whitespace().nth(1) {
            Some("200") => Ok(()),
            Some(_) => Err(format!("answered {}", status.trim())),
            None if status.trim().is_empty() => Err("closed the connection unanswered".into()),
            None => Err(format!("answered {:?}", status.trim())),
        },
        Err(e) => Err(io_words(&e, &wait)),
    }
}

/// What a failed read or write on an engine's unix socket did, in words,
/// an answer waited for up to `wait`.
fn io_words(e: &std::io::Error, wait: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => "refused the connection".into(),
        std::io::ErrorKind::PermissionDenied => "refused this user".into(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            format!("gave no answer within {wait}")
        }
        _ => e.to_string(),
    }
}

/// `GET /_ping` at the TCP address `host:port`, plain or over `tls`: `Ok`
/// when it answers `200`, else what it did, in words. The look is one try:
/// it asks the host's engine, not a remote (rule core.29).
fn ping_tcp(host: &str, port: u16, tls: Option<&EngineTls>) -> Result<(), String> {
    let wait = format!("{}s", ENGINE_WAIT.as_secs());
    let mut config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(ENGINE_WAIT))
        .timeout_global(Some(ENGINE_WAIT * 3))
        .user_agent("sluice");
    let scheme = match tls {
        Some(t) => {
            config = config.tls_config(engine_tls_config(t)?);
            "https"
        }
        None => "http",
    };
    let agent: ureq::Agent = config.build().into();
    match agent.get(&format!("{scheme}://{host}:{port}/_ping")).call() {
        Ok(r) if r.status() == 200 => Ok(()),
        Ok(r) => Err(format!("answered {}", r.status())),
        Err(e) => Err(tcp_words(e, &wait)),
    }
}

/// What a failed call to an engine at a TCP address did, in words, an
/// answer waited for up to `wait` once [`ENGINE_WAIT`] gave the
/// connection.
fn tcp_words(e: ureq::Error, wait: &str) -> String {
    match e {
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            "refused the connection".into()
        }
        ureq::Error::ConnectionFailed => "refused the connection".into(),
        ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect) => {
            format!("took no connection within {}s", ENGINE_WAIT.as_secs())
        }
        ureq::Error::Timeout(_) => format!("gave no answer within {wait}"),
        ureq::Error::HostNotFound => "names no host that resolves".into(),
        // A TLS failure is told in TLS's own words.
        ureq::Error::Io(io) => io.to_string(),
        other => other.to_string(),
    }
}

/// The TLS the engine's client speaks (design D5): its certificate and key
/// from `cert.pem` and `key.pem` where they are, and the engine verified
/// against `ca.pem` where `verify` asks, else not verified.
fn engine_tls_config(tls: &EngineTls) -> Result<ureq::tls::TlsConfig, String> {
    use ureq::tls::{parse_pem, ClientCert, PemItem, RootCerts, TlsConfig};
    let read = |name: &str| {
        let path = tls.certs.join(name);
        std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))
    };
    let certs = |pem: &[u8]| {
        parse_pem(pem)
            .filter_map(|i| match i {
                Ok(PemItem::Certificate(c)) => Some(c),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let mut config = TlsConfig::builder();
    if let (Ok(cert), Ok(key)) = (read("cert.pem"), read("key.pem")) {
        let key = parse_pem(&key).find_map(|i| match i {
            Ok(PemItem::PrivateKey(k)) => Some(k),
            _ => None,
        });
        if let Some(key) = key {
            config = config.client_cert(Some(ClientCert::new_with_certs(&certs(&cert), key)));
        }
    }
    config = match tls.verify {
        true => config.root_certs(RootCerts::new_with_certs(&certs(&read("ca.pem")?))),
        false => config.disable_verification(true),
    };
    Ok(config.build())
}

/// A path by the path given and, where it differs, the path it resolves
/// to through its nearest directory that exists ([`resolved`]): Seatbelt
/// matches resolved paths, and `/var` is `/private/var`, so a path not
/// made yet under a link is matched where it will lie once made (record
/// 0105). sluice makes nothing of it.
fn given_and_resolved(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    let real = resolved(path);
    if real != path {
        paths.push(real);
    }
    paths
}

/// The Seatbelt profile of a run in `place`, and its grant: its checkout
/// and scratch directory writable, each `dir` of the host's `caches`, and
/// the runner's `store`, where moon keeps what it shares for the run,
/// where it lies outside them (C.2 sandbox row as amended at
/// agentplot/blueprints 0c38f24; record 0105; rule core.9); the container
/// engine's `engines`, its sockets and its TCP addresses, reachable when
/// `limits` allow it (ADR 0016; design D5), and each `socket` of the
/// caches, whatever the network (35; design D4). The grant names the
/// store a `<cache>`, as a declared cache is (design D1). Each refusal
/// rule carries `mark`, the run's (139; design D4 of the change
/// `a-check-its-executor-refused-has-no-outcome`).
fn sandbox_profile(
    place: &Place,
    limits: &Limits,
    stamps: &[&str],
    caches: &[HostCache],
    store: Option<&Path>,
    engines: Vec<Endpoint>,
    mark: Option<&str>,
) -> Profile {
    Reach {
        mark: mark.map(str::to_string),
        ..sandbox_reach(place, limits, stamps, caches, store, engines, user_tmp())
    }
    .profile()
}

/// What [`sandbox_profile`] writes its profile from, with `user_tmp` the
/// user's own temporary directory.
fn sandbox_reach(
    place: &Place,
    limits: &Limits,
    stamps: &[&str],
    caches: &[HostCache],
    store: Option<&Path>,
    engines: Vec<Endpoint>,
    user_tmp: Option<PathBuf>,
) -> Reach {
    let mut writable = vec![
        (Role::Place, resolved(&place.path)),
        (Role::Scratch, resolved(&place.tmp)),
    ];
    // Record 0105: the store the run records, the one its proof is read
    // from; a store in the checkout or its scratch adds nothing, so such
    // a run's profile is the one it is given with none (rule core.34).
    if let Some(store) = store {
        let real = resolved(store);
        if !writable.iter().any(|(_, w)| real.starts_with(w)) {
            writable.extend(
                given_and_resolved(store)
                    .into_iter()
                    .map(|p| (Role::Cache, p)),
            );
        }
    }
    let engines = if limits.docker { engines } else { Vec::new() };
    let mut engine_sockets = Vec::new();
    let mut tcp = Vec::new();
    for e in engines {
        match e {
            Endpoint::Unix(p) => engine_sockets.push(p),
            Endpoint::Tcp { host, port, .. } => tcp.push((host, port)),
        }
    }
    let mut cache_sockets = Vec::new();
    for cache in caches {
        writable.extend(
            given_and_resolved(&cache.dir)
                .into_iter()
                .map(|p| (Role::Cache, p)),
        );
        if let Some(s) = &cache.socket {
            cache_sockets.extend(given_and_resolved(s));
        }
    }
    engine_sockets.sort();
    engine_sockets.dedup();
    cache_sockets.sort();
    cache_sockets.dedup();
    cache_sockets.retain(|s| !engine_sockets.contains(s));
    let sockets = engine_sockets
        .into_iter()
        .map(|p| (Role::EngineSocket, p))
        .chain(cache_sockets.into_iter().map(|p| (Role::CacheSocket, p)))
        .collect();
    Reach {
        writable,
        stamps: stamps.iter().map(|s| s.to_string()).collect(),
        network: limits.network,
        sockets,
        tcp,
        user_tmp,
        mark: None,
    }
}

/// Design D4: a run's mark, `sluice-run:` and 16 hex characters of the
/// SHA-256 of its directory and the time it started, so no two runs share
/// one.
fn run_mark(dir: &Path, started_ms: i64) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(dir.as_os_str().as_encoded_bytes());
    h.update(b"\0");
    h.update(started_ms.to_string().as_bytes());
    let hex: String = h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sluice-run:{hex}")
}

/// Design D5: the watch a sandboxed run's supervisor keeps its sandbox's
/// reports with, `log` streaming every report of `mark`.
fn watch_argv(log: &Path, mark: &str) -> Vec<String> {
    vec![
        log.to_string_lossy().into_owned(),
        "stream".into(),
        "--style".into(),
        "ndjson".into(),
        "--predicate".into(),
        format!("sender == \"Sandbox\" AND eventMessage CONTAINS \"{mark}\""),
    ]
}

/// The program a sandboxed run's reports are streamed with on the host.
const LOG: &str = "/usr/bin/log";

/// `[tests] release`: the command, or `None`.
fn release_argv(tests: &Settings) -> Result<Option<Vec<String>>> {
    command_setting(tests, "release")
}

/// A command `[tests]` names under `key`: a list, the command first, or a
/// string run by `sh -c`.
fn command_setting(tests: &Settings, key: &str) -> Result<Option<Vec<String>>> {
    match tests.get(key) {
        None => Ok(None),
        Some(Setting::Text(t)) => Ok(Some(vec!["/bin/sh".into(), "-c".into(), t.clone()])),
        Some(Setting::List(xs)) if !xs.is_empty() => Ok(Some(xs.clone())),
        Some(_) => bail!("[tests] {key} must be a string or a list, the command first"),
    }
}

/// `[tests] not_run`: a number or a list of them.
fn not_run_exits(tests: &Settings) -> Result<Vec<i32>> {
    let parse = |s: &str| {
        s.parse::<i32>()
            .with_context(|| format!("[tests] not_run lists exits, not {s:?}"))
    };
    match tests.get("not_run") {
        None => Ok(Vec::new()),
        Some(Setting::Text(t)) => Ok(vec![parse(t)?]),
        Some(Setting::List(xs)) => xs.iter().map(|x| parse(x)).collect(),
        Some(_) => bail!("[tests] not_run must be an exit or a list of them"),
    }
}

/// Session-runs-1 clause 95: the release command of `place` as the
/// policy's `[tests]` and limits state it now, under the same executor,
/// toolchain and environment its runs had, ports and the host's `caches`
/// included; `None` when the policy names no release command. What the
/// toolchain and the executor write goes in `dir`.
pub fn release_request(
    place: &Place,
    tests: &Settings,
    limits: &Limits,
    jobs: usize,
    caches: &[HostCache],
    dir: &Path,
) -> Result<Option<Request>> {
    let Some(release) = release_argv(tests)? else {
        return Ok(None);
    };
    let tool_argv = toolchain_argv(tests)?;
    let kind = choose(limits)?;
    let request = RunRequest {
        place: place.clone(),
        revision: sluice_seams::Rev::new(""),
        required: Vec::new(),
        since: None,
        limit: None,
        limits: limits.clone(),
        tests: tests.clone(),
        command: None,
        replay: false,
        system: None,
    };
    let mut env = environment(&request, jobs, caches);
    if kind == Kind::Sandbox {
        env.insert("TMPDIR".into(), place.tmp.to_string_lossy().into());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let script = dir.join("toolchain.sh");
    // 95: the working copy was equipped by the run it releases; the
    // release enters its toolchain alone.
    if let Some(argv) = &tool_argv {
        let step = toolchain(argv, &place.path, &env, &script)?;
        if !step.passed {
            bail!("{}: {}", step.step, step.words);
        }
    }
    let cx = Context {
        request: &request,
        env: &env,
        toolchain: tool_argv.is_some().then_some(script.as_path()),
        jobs,
    };
    let r: Vec<&str> = release.iter().map(String::as_str).collect();
    let mut argv = cx.argv(&r);
    if kind == Kind::Sandbox {
        let profile = dir.join("sandbox.sb");
        let engines = match limits.docker {
            true => engine_endpoints(),
            false => Vec::new(),
        };
        std::fs::write(
            &profile,
            // A release runs no check whose proof sluice reads.
            sandbox_profile(place, limits, &[], caches, None, engines, None).text,
        )?;
        argv.splice(
            0..0,
            [
                "/usr/bin/sandbox-exec".to_string(),
                "-f".into(),
                profile.to_string_lossy().into_owned(),
            ],
        );
    }
    Ok(Some(Request {
        argv: Vec::new(),
        cwd: place.path.clone(),
        env,
        limit: None,
        grace: 10,
        meta: serde_json::Value::Null,
        release: Some(argv),
        watch: None,
    }))
}

/// A scratch directory removed when it is dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 140: a detached worktree of the repository whose common directory is
/// `common`, made with no hook run, for the runner's affected reading
/// alone, and removed when it is dropped. No place: nothing runs in it but
/// the runner's query.
struct ReadingTree {
    common: PathBuf,
    path: PathBuf,
}

impl ReadingTree {
    fn make(common: &Path, path: &Path, rev: &Rev) -> Result<ReadingTree> {
        let tree = ReadingTree {
            common: common.to_path_buf(),
            path: path.to_path_buf(),
        };
        let at = path
            .to_str()
            .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))?;
        let out = tree.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--detach",
            at,
            rev.as_str(),
        ])?;
        if !out.status.success() {
            bail!(
                "git worktree add {at}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(tree)
    }

    fn git(&self, args: &[&str]) -> Result<std::process::Output> {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&self.common)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .stdin(std::process::Stdio::null())
            .output()
            .context("running git")
    }
}

impl Drop for ReadingTree {
    fn drop(&mut self) {
        if let Some(at) = self.path.to_str() {
            let _ = self.git(&["worktree", "remove", "--force", at]);
        }
        let _ = std::fs::remove_dir_all(&self.path);
        let _ = self.git(&["worktree", "prune"]);
    }
}

pub fn hostname() -> String {
    nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "localhost".into())
}

impl Tests for Checks {
    fn start(&self, request: &RunRequest, part: Part) -> Result<RunHandle> {
        // 71 (rule core.49): the room its part holds, or none it honours.
        let Ok(Holding(mut admission)) = part.take::<Holding>() else {
            return Err(AtBound.into());
        };
        let runner = text(&request.tests, "runner")?
            .ok_or_else(|| anyhow!("[tests] names no runner: \"moon\" or \"command\""))?
            .to_string();
        toolchain_argv(&request.tests)?;
        // 84: refuse before anything runs.
        let kind = choose(&request.limits)?;
        outside_checkout(&request.place.path, &self.instance.caches)?;
        let jobs = self.instance.jobs();
        let run_env = |caches: &[HostCache]| {
            let mut env = environment(request, jobs, caches);
            // ADR 0014: what the checks export is found beside the round.
            env.extend(crate::telemetry::run_env(&[
                ("sluice.place", &request.place.name),
                ("sluice.revision", request.revision.as_str()),
            ]));
            if kind == Kind::Sandbox {
                env.insert("TMPDIR".into(), request.place.tmp.to_string_lossy().into());
            }
            env
        };
        let env = run_env(&self.instance.caches);
        // A session's place takes a new run each time, and keeps the last
        // (session-runs-1 clause 94).
        let session = crate::runs::is_session(&request.place.name);
        let name = &if session {
            self.runs.next_run(&request.place.name)?
        } else {
            request.place.name.clone()
        };
        let dir = self.runs.prepare(name)?;
        // 71: its part's entry names the run from now, its hold keeping it
        // counted until the run's supervisor holds the run directory.
        admission.begin(&request.place.name, &dir)?;
        // 84, record 0097 (design D2): the host's system as the run
        // begins, before its checkout is equipped, so a switch while it is
        // equipped is one during the run.
        let system = nix_system(&self.system_link());
        let script = dir.join("toolchain.sh");
        // 147: equipped up to the version check before the runner is
        // asked anything, and through the setup only where it runs a check.
        let mut equipping = Equipping::new(request, &env, jobs, &script)?;
        equipping.ready(Through::Reading)?;
        let cx = equipping.context();
        let sourced = cx.toolchain.is_some();
        let not_run = not_run_exits(&request.tests)?;
        // 89, 98: a request's command runs in place of the runner's
        // checks, in a working copy equipped by the runner `[tests]`
        // names (147), and plans as the command runner does.
        let command = match (&request.command, runner.as_str()) {
            (Some(command), _) => Some(command.as_str()),
            (None, "command") => Some(
                text(&request.tests, "command")?
                    .ok_or_else(|| anyhow!("[tests] runner = \"command\" needs a command"))?,
            ),
            (None, _) => None,
        };
        let plan = match (command, runner.as_str()) {
            (None, "moon") => Moon.plan(&cx)?,
            (Some(command), _) => {
                Plan {
                    argv: Some(cx.argv(&["/bin/sh", "-c", command])),
                    targets: vec![command.to_string()],
                    holds_processors: false,
                    kept_out: Vec::new(),
                    deps: BTreeMap::new(),
                    // The command is the runner's one test.
                    cold: cold_tests(&request.tests)?.iter().any(|c| c == command),
                    // A command in a checkout that lists proto's tools may
                    // run them, moon among them, and proto stamps each
                    // toolchain it runs as used, as a moon run's does.
                    stamps: if request.place.path.join(".prototools").exists() {
                        vec![".last-used"]
                    } else {
                        Vec::new()
                    },
                }
            }
            (None, other) => {
                bail!("[tests] runner = {other:?} names no runner: \"moon\" or \"command\"")
            }
        };
        if plan.argv.is_some() {
            equipping.ready(Through::Running)?;
        }
        // What the run's results, progress and logs are read by.
        let runner = match command {
            Some(_) => "command".to_string(),
            None => runner,
        };
        let processors = plan.holds_processors.then_some(jobs);
        let mut argv = plan.argv.unwrap_or_else(|| vec!["/usr/bin/true".into()]);
        let mut release = release_argv(&request.tests)?.map(|r| {
            let r: Vec<&str> = r.iter().map(String::as_str).collect();
            cx.argv(&r)
        });
        // 35: a run holding a cold test, and its release, are given none of
        // the host's caches. What ran to set it up, the toolchain and the
        // runner's own queries, had them and ran no test (design D5).
        let caches: &[HostCache] = if plan.cold {
            &[]
        } else {
            &self.instance.caches
        };
        let env = if plan.cold { run_env(caches) } else { env };
        // Design D8 (C.2 moon, 35): where moon keeps this run's manifests,
        // as moon finds it when it starts, from the environment its command
        // starts in: the run's, cold or not, with the toolchain sourced.
        let store = match runner.as_str() {
            "moon" => {
                let command = Context {
                    request,
                    env: &env,
                    toolchain: sourced.then_some(script.as_path()),
                    jobs,
                };
                Some(super::moon::store(
                    &request.place.path,
                    &command.command_env()?,
                ))
            }
            _ => None,
        };
        let docker = request.limits.docker;
        let started_ms = chrono::Utc::now().timestamp_millis();
        // Design D4, D5: a sandboxed run's mark, and the watch that keeps
        // its sandbox's reports of it.
        let (mark, watch) = match kind {
            Kind::Open => (None, None),
            Kind::Sandbox => {
                let mark = run_mark(&dir, started_ms);
                let log = self.log.clone().unwrap_or_else(|| PathBuf::from(LOG));
                let watch = watch_argv(&log, &mark);
                (Some(mark), Some(watch))
            }
        };
        let executor = match kind {
            Kind::Open => Enforced {
                kind: "open".into(),
                confined: false,
                processors,
                memory: None,
                network: None,
                docker,
                // Design D1: every open run's.
                grant: Some("open:1".into()),
            },
            Kind::Sandbox => {
                let network = request.limits.network;
                let profile = dir.join("sandbox.sb");
                let written = sandbox_profile(
                    &request.place,
                    &request.limits,
                    &plan.stamps,
                    caches,
                    // Record 0105: the store the run's record names,
                    // and its release's, which runs under this profile.
                    store.as_deref(),
                    match request.limits.docker {
                        true => self.engines(),
                        false => Vec::new(),
                    },
                    mark.as_deref(),
                );
                std::fs::write(&profile, &written.text)?;
                let sandbox = [
                    "/usr/bin/sandbox-exec".to_string(),
                    "-f".into(),
                    profile.to_string_lossy().into_owned(),
                ];
                argv.splice(0..0, sandbox.clone());
                if let Some(r) = &mut release {
                    r.splice(0..0, sandbox);
                }
                // A session on this host can still write the place; and a
                // run that reaches the container engine is held by nothing.
                Enforced {
                    kind: "sandbox".into(),
                    confined: false,
                    processors,
                    memory: None,
                    network: network.filter(|n| *n != Network::Allowed),
                    docker,
                    // Design D1: the profile it ran under, by role.
                    grant: Some(written.grant),
                }
            }
        };
        let meta = Meta {
            runner,
            revision: request.revision.to_string(),
            place: request.place.path.clone(),
            targets: plan.targets,
            executor,
            host: hostname(),
            started_ms: Some(started_ms),
            not_run,
            kept_out: plan.kept_out,
            deps: plan.deps,
            store,
            nix_system: system,
            mark,
        };
        self.runs.start(
            name,
            &Request {
                // Nothing affected: nothing of the repository's is run.
                argv,
                cwd: request.place.path.clone(),
                env,
                limit: request.limit.map(|d| d.as_secs().max(1)),
                grace: 10,
                meta: serde_json::to_value(&meta)?,
                release,
                watch,
            },
        )?;
        // Its supervisor holds the run directory, which keeps it counted.
        admission.started();
        Ok(RunHandle(name.clone()))
    }

    /// 84, 87, record 0074 (design D2): a limit the executor cannot hold,
    /// in the words its start would refuse it with, before any socket is
    /// looked at; then, for a run that may reach the container engine, one
    /// that answers where the run would reach it.
    fn unmet(&self, limits: &Limits) -> Result<Option<String>> {
        if let Err(refused) = choose(limits) {
            return Ok(Some(format!("{refused:#}")));
        }
        if !limits.docker {
            return Ok(None);
        }
        Ok(engine_unmet(&self.engines()))
    }

    /// 84, 114, record 0097 (design D3, D4): the system the run noted as
    /// it began against the one the link names now; nothing read where the
    /// run noted none.
    fn switched(&self, run: &RunHandle) -> Result<Option<String>> {
        let request = self.runs.request(&run.0)?;
        let meta: Meta = serde_json::from_value(request.meta.clone())
            .with_context(|| format!("reading run {}", run.0))?;
        let Some(began) = meta.nix_system else {
            return Ok(None);
        };
        Ok(switch(
            Some(&began),
            nix_system(&self.system_link()).as_deref(),
        ))
    }

    /// 139, record 0072 (design D6): what the sandbox refused the run,
    /// read from the reports of the run's mark its watch kept, and from
    /// nothing a check printed (rule core.35). An open run, and a run an
    /// older sluice started, which carry no mark, answer `None`; a watch
    /// that was not ready, or a report that cannot be read, `Unread` with
    /// its words. It runs nothing, so reading a run again reads the same.
    fn refused(&self, run: &RunHandle) -> Result<Refusals> {
        let request = self.runs.request(&run.0)?;
        let meta: Meta = serde_json::from_value(request.meta.clone())
            .with_context(|| format!("reading run {}", run.0))?;
        let Some(mark) = meta.mark else {
            return Ok(Refusals::None);
        };
        let dir = self.runs.dir(&run.0);
        let said = dir.join("watch-error");
        match std::fs::read_to_string(&said) {
            Ok(words) => return Ok(Refusals::Unread(words.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Ok(Refusals::Unread(format!("reading {}: {e}", said.display()))),
        }
        let path = dir.join("watch.out");
        let kept = match std::fs::read(&path) {
            Ok(k) => String::from_utf8_lossy(&k).into_owned(),
            Err(e) => return Ok(Refusals::Unread(format!("reading {}: {e}", path.display()))),
        };
        let refusals = super::refusals::read(&kept, &mark);
        Ok(match refusals.is_empty() {
            true => Refusals::None,
            false => Refusals::Refused(super::refusals::words(&refusals)),
        })
    }

    fn progress(&self, run: &RunHandle) -> Result<Progress> {
        let ended = match self.runs.state(&run.0)? {
            RunState::Running => return Ok(Progress::Running),
            RunState::Lost => return Ok(Progress::Lost),
            RunState::Finished(e) => e,
        };
        let request = self.runs.request(&run.0)?;
        let meta: Meta = serde_json::from_value(request.meta.clone())
            .with_context(|| format!("reading run {}", run.0))?;
        // Could-not-run-1 114: a runner that never started judged nothing.
        let mut detail = None;
        let mut dependencies = Vec::new();
        let mut failed_by = Vec::new();
        let (mut tasks, not_run) = match &ended.error {
            Some(e) => (
                Vec::new(),
                meta.targets
                    .iter()
                    .map(|t| NotRun {
                        task: t.clone(),
                        reason: e.clone(),
                        key: None,
                    })
                    .collect(),
            ),
            None => match meta.runner.as_str() {
                "moon" => {
                    // 66: in a run its time limit ended, nothing is read
                    // through a dependency, since one the limit ended
                    // reports the exit it took on the signal, which is no
                    // fail (149).
                    let none = BTreeMap::new();
                    let deps = if ended.timed_out { &none } else { &meta.deps };
                    let read =
                        Moon.read(&meta.place, &meta.targets, deps, &meta.not_run, &meta.host)?;
                    detail = read.detail;
                    dependencies = read.dependencies;
                    failed_by = read.failed_by;
                    (read.tasks, read.not_run)
                }
                _ => command_result(&meta, &ended),
            },
        };
        let missing: Vec<String> = if ended.timed_out {
            // Time-limit-1 66: what ended before the limit keeps its
            // outcome; a task the limit cut off has none, and is named as
            // left without one. One never started is not named.
            match (meta.runner.as_str(), meta.started_ms, request.limit) {
                ("moon", Some(started), Some(limit)) => {
                    let out =
                        std::fs::read(self.runs.dir(&run.0).join("out.log")).unwrap_or_default();
                    let judged: Vec<String> = meta
                        .targets
                        .iter()
                        .filter(|t| !not_run.iter().any(|n| &n.task == *t))
                        .cloned()
                        .collect();
                    let limit_ms = started.saturating_add(
                        i64::try_from(limit)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(1000),
                    );
                    let cut = Moon.cut_off(&meta.place, &out, &judged, &tasks, started, limit_ms);
                    tasks.retain(|t| !cut.contains(&t.task));
                    cut
                }
                _ => Vec::new(),
            }
        } else {
            meta.targets
                .iter()
                .filter(|t| !tasks.iter().any(|r| &r.task == *t))
                .filter(|t| !not_run.iter().any(|n| &n.task == *t))
                .filter(|t| !failed_by.iter().any(|f| &f.task == *t))
                .cloned()
                .collect()
        };
        Ok(Progress::Finished(RunReport {
            detail,
            kept_out: meta.kept_out,
            tasks,
            executor: meta.executor,
            missing,
            not_run,
            dependencies,
            failed_by,
            timed_out: ended.timed_out,
            refusals_unread: None,
        }))
    }

    fn hold(&self, kind: RunKind) -> Result<Option<Part>> {
        self.admit(kind)
    }

    fn bound(&self) -> Result<usize> {
        Ok(self.instance.bound)
    }

    fn launch(&self, launch: &sluice_seams::Launch) -> Result<()> {
        match &self.pool {
            Some(p) => p.launch(launch),
            None => anyhow::bail!(
                "this host has no pool of kind {} for {}: name the app it reaches GitHub as under [forge]",
                launch.kind.0,
                launch.pool
            ),
        }
    }

    fn job_alive(
        &self,
        kind: &(String, Setting),
        claim: &str,
        remote: Option<&str>,
    ) -> Result<bool> {
        match &self.pool {
            Some(p) => p.job_alive(kind, claim, remote),
            // A host that cannot ask the pool reads its jobs as running:
            // a claim lapses only by what is known of it (77).
            None => Ok(true),
        }
    }

    fn wake(&self, kind: &(String, Setting), branch: &str, remote: Option<&str>) -> Result<()> {
        match &self.pool {
            Some(p) => p.wake(kind, branch, remote),
            None => Ok(()),
        }
    }

    fn platform(&self) -> Result<sluice_seams::Platform> {
        let (os, arch) = crate::instance::this_system();
        Ok(sluice_seams::Platform {
            os: os.into(),
            arch: arch.into(),
        })
    }

    fn running(&self) -> Result<Running> {
        self.host_count()?.running()
    }

    /// A command runner's one key is sluice's own, so every task is
    /// proven. A moon task's key is checked against the candidate's tree
    /// by its manifest, read where the run's moon kept it, in the checkout
    /// or in the shared store moon's own settings place it (C.2 moon row;
    /// rule core.9): each manifest the walk needs, the key's own and each
    /// dependency's, is read in the store the run's record names and,
    /// where that store is not the checkout's own `.moon/cache` and lacks
    /// it, in the checkout's own, which this run's moon alone writes
    /// (design D9; design D1 of the change
    /// `a-signed-run-without-a-sandbox-proves-its-passes`). Either is held
    /// to the same proof by its content.
    ///
    /// A ran check that passed whose manifest, or a dependency's, is
    /// missing there is unkept: the run kept no proof of it, which says
    /// nothing of the work, so it is not warned of and has no outcome
    /// (114; record 0105; the change `a-pass-nothing-proves-blames-no-one`,
    /// design D2 to D4). It names the missing manifest's key, and its
    /// reason whose manifest is missing, the record's store and what that
    /// store says of itself, read from the store with nothing run, never
    /// from what moon printed (rule core.35; design D3 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`). A fail, a task
    /// a check was failed by (149) and a replay (108) are never unkept.
    ///
    /// Every other key the check refuses is one the tree does not bear
    /// out, in neither list and warned of; one refused for a cause other
    /// than a missing manifest is answered in `refused` with its key and
    /// the check's words for why, so the submission blamed for it says why
    /// (C.6, 20; design D5).
    fn proven(&self, run: &RunHandle, report: &RunReport) -> Result<Proof> {
        let meta: Meta = serde_json::from_value(self.runs.request(&run.0)?.meta)
            .with_context(|| format!("reading run {}", run.0))?;
        let tasks = report.tasks.iter().chain(&report.dependencies);
        if meta.runner != "moon" {
            return Ok(Proof {
                proven: tasks.map(|t| t.task.clone()).collect(),
                ..Proof::default()
            });
        }
        let own = meta.place.join(".moon/cache");
        let store = meta.store.clone().unwrap_or_else(|| own.clone());
        let blob = |k: &str| {
            super::moon::blob_in(&store, k).or_else(|| {
                (store != own)
                    .then(|| super::moon::blob_in(&own, k))
                    .flatten()
            })
        };
        let tree = |p: &str| super::moon::file_at(&meta.place, &meta.revision, p);
        // A task a check was failed by is no check of the run (149).
        let checks = report.tasks.iter().map(|t| (t, true));
        let dependencies = report.dependencies.iter().map(|t| (t, false));
        let mut proof = Proof::default();
        // Design D3 of `a-signed-run-without-a-sandbox-proves-its-passes`:
        // how many of the run's keys moon kept a manifest of, in either
        // store, read once a pass is found unkept.
        let kept = std::cell::OnceCell::new();
        for (t, check) in checks.chain(dependencies) {
            let e = match super::moon::check_key(&t.hash, &t.task, &blob, &tree) {
                Ok(()) => {
                    proof.proven.push(t.task.clone());
                    continue;
                }
                Err(e) => e,
            };
            let ran_pass =
                check && matches!(t.source, Source::Ran { .. }) && t.outcome == Outcome::Pass;
            let missing = e.downcast_ref::<moon::NoManifest>().filter(|_| ran_pass);
            if let Some(missing) = missing {
                let whose = if missing.task == t.task {
                    "it".to_string()
                } else {
                    format!("{}, which it depends on", missing.task)
                };
                let kept = *kept.get_or_init(|| {
                    report
                        .tasks
                        .iter()
                        .chain(&report.dependencies)
                        .filter(|t| blob(&t.hash).is_some())
                        .count()
                });
                let fact = super::moon::store_fact(&store, &missing.key, kept);
                proof.unkept.push(NotRun {
                    task: t.task.clone(),
                    reason: format!(
                        "it passed, but nothing the run kept proves it ran: \
                         moon's manifest of {whose} is not in {}, {fact}",
                        store.display()
                    ),
                    key: Some(missing.key.clone()),
                });
                continue;
            }
            if e.downcast_ref::<moon::NoManifest>().is_none() {
                proof.refused.push(sluice_seams::Refused {
                    task: t.task.clone(),
                    key: t.hash.clone(),
                    why: format!("{e:#}"),
                });
            }
            tracing::warn!(
                sluice.task = %t.task,
                sluice.hash = %t.hash,
                "a key the combination's tree does not bear out is not signed: {e:#}"
            );
        }
        Ok(proof)
    }

    fn session_runs(&self) -> Result<Vec<HostRun>> {
        Ok(self
            .host_sessions()?
            .running()?
            .into_iter()
            .map(|r| HostRun {
                worktree: r.worktree,
                branch: r.branch,
                started: r.started,
                command: r.command,
                tasks: r.tasks,
            })
            .collect())
    }

    fn logs(&self, run: &RunHandle, tasks: &[String]) -> Result<Vec<TaskLog>> {
        let meta: Meta = serde_json::from_value(self.runs.request(&run.0)?.meta)
            .with_context(|| format!("reading run {}", run.0))?;
        let out = std::fs::read(self.runs.dir(&run.0).join("out.log")).unwrap_or_default();
        Ok(tasks
            .iter()
            .map(|task| {
                let own = match meta.runner.as_str() {
                    "moon" => Moon.log(&meta.place, task),
                    _ => None,
                };
                TaskLog {
                    task: task.clone(),
                    text: log_tail(own.as_deref().unwrap_or(&out)),
                }
            })
            .collect())
    }

    /// ADR 0050: moon asked in the worktree as a run's place is set up,
    /// the run's environment and `[tests] toolchain` included, with the
    /// toolchain's script in a scratch directory of sluice's state, gone
    /// once moon has answered. Outside any executor, as `plan`'s own
    /// `moon query` is. A command runner keeps nothing out.
    fn kept_out(
        &self,
        worktree: &Path,
        tests: &Settings,
        required: &[String],
    ) -> Result<Vec<KeptOutTask>> {
        if text(tests, "runner")? != Some("moon") {
            return Ok(Vec::new());
        }
        let scratch = Scratch(self.runs.dir(&format!("kept-out-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&scratch.0);
        let tmp = scratch.0.join("tmp");
        std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let request = RunRequest {
            place: Place {
                name: "kept-out".into(),
                path: worktree.to_path_buf(),
                tmp,
                env: BTreeMap::new(),
            },
            revision: sluice_seams::Rev::new(""),
            required: required.to_vec(),
            since: None,
            limit: None,
            limits: Limits::default(),
            tests: tests.clone(),
            command: None,
            replay: false,
            system: None,
        };
        let jobs = self.instance.jobs();
        let env = environment(&request, jobs, &self.instance.caches);
        let script = scratch.0.join("toolchain.sh");
        let mut equipping = Equipping::new(&request, &env, jobs, &script)?;
        equipping.ready(Through::Reading)?;
        Moon.kept_out(&equipping.context())
    }

    /// 140: the moon runner's affected reading, in a worktree of the first
    /// change's second revision made for it under the check seam's own
    /// directory and removed once moon has answered, with the run's
    /// environment and `[tests] toolchain` set up as `kept_out` sets them
    /// up, outside any executor. It takes no place and is admitted against
    /// no bound: it runs no check. The command runner's one check reads
    /// everything.
    fn reads(&self, reading: &Reading) -> Result<Vec<Reads>> {
        if text(&reading.tests, "runner")? != Some("moon") || reading.pairs.is_empty() {
            return Ok(vec![Reads::Every; reading.pairs.len()]);
        }
        let scratch = Scratch(self.runs.dir(&format!("reading-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&scratch.0);
        let tmp = scratch.0.join("tmp");
        std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let at = reading.pairs[0].1.clone();
        let tree = ReadingTree::make(&self.common, &scratch.0.join("tree"), &at)?;
        let request = RunRequest {
            place: Place {
                name: "reading".into(),
                path: tree.path.clone(),
                tmp,
                env: BTreeMap::new(),
            },
            revision: at,
            required: reading.required.clone(),
            since: None,
            limit: None,
            limits: Limits::default(),
            tests: reading.tests.clone(),
            command: None,
            replay: false,
            system: None,
        };
        let jobs = self.instance.jobs();
        let env = environment(&request, jobs, &self.instance.caches);
        let script = scratch.0.join("toolchain.sh");
        let mut equipping = Equipping::new(&request, &env, jobs, &script)?;
        equipping.ready(Through::Reading)?;
        Moon.reads(&equipping.context(), &reading.pairs)
    }

    /// 147: `equip.working_copy` equipped through `equip.through` in a
    /// scratch directory of the check seam's own, `.runs/equip-<pid>/`,
    /// removed before it answers, with the environment a run there starts
    /// from, of a place named `equip` with no ports. It takes no place and
    /// is admitted against no bound (71): it runs no check.
    fn equip(&self, equip: &Equip) -> Result<Equipped> {
        let scratch = Scratch(self.runs.dir(&format!("equip-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&scratch.0);
        let tmp = scratch.0.join("tmp");
        std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let request = RunRequest {
            place: Place {
                name: "equip".into(),
                path: equip.working_copy.clone(),
                tmp,
                env: BTreeMap::new(),
            },
            revision: sluice_seams::Rev::new(""),
            required: Vec::new(),
            since: None,
            limit: None,
            limits: Limits::default(),
            tests: equip.tests.clone(),
            command: None,
            replay: false,
            system: None,
        };
        let jobs = self.instance.jobs();
        let env = environment(&request, jobs, &self.instance.caches);
        let script = scratch.0.join("toolchain.sh");
        let mut equipping = Equipping::new(&request, &env, jobs, &script)?;
        equipping.through(equip.through)?;
        Ok(equipping.equipped())
    }

    /// ADR 0028: the moon runner's tasks while the run goes; the command
    /// runner's one task is the run itself, so it says nothing.
    fn tasks(&self, run: &RunHandle) -> Result<Vec<TaskProgress>> {
        if !matches!(self.runs.state(&run.0)?, RunState::Running) {
            return Ok(Vec::new());
        }
        let meta: Meta = serde_json::from_value(self.runs.request(&run.0)?.meta)
            .with_context(|| format!("reading run {}", run.0))?;
        let (Some(started), "moon") = (meta.started_ms, meta.runner.as_str()) else {
            return Ok(Vec::new());
        };
        let out = std::fs::read(self.runs.dir(&run.0).join("out.log")).unwrap_or_default();
        let times = self.runs.times(&run.0).unwrap_or_default();
        Ok(Moon.progress(&meta.place, &out, &times, &meta.targets, started))
    }
}

/// The `command` runner's one check: its key is the command and the
/// candidate, its outcome the exit; a run ended by its time limit
/// observed nothing, and an exit `[tests] not_run` names judged nothing
/// (could-not-run-1 114).
fn command_result(meta: &Meta, ended: &Ended) -> (Vec<TaskResult>, Vec<NotRun>) {
    if ended.timed_out {
        return (Vec::new(), Vec::new());
    }
    if let Some(code) = ended.exit.filter(|c| meta.not_run.contains(c)) {
        let not_run = meta
            .targets
            .iter()
            .map(|t| NotRun {
                task: t.clone(),
                reason: not_run_reason(code),
                key: None,
            })
            .collect();
        return (Vec::new(), not_run);
    }
    let tasks = meta
        .targets
        .iter()
        .map(|t| TaskResult {
            took_ms: took_ms(&ended.started, &ended.ended),
            attempts: None,
            task: t.clone(),
            hash: meta.revision.clone(),
            outcome: if ended.passed() {
                Outcome::Pass
            } else {
                Outcome::Fail
            },
            source: Source::Ran {
                host: meta.host.clone(),
            },
        })
        .collect();
    (tasks, Vec::new())
}

/// Why a check that exited `code` could not run.
pub fn not_run_reason(code: i32) -> String {
    format!("exited {code}, which [tests] not_run names: it could not run here")
}

/// Clause 119: milliseconds between two RFC 3339 times,
/// the command runner's time for its one command.
fn took_ms(started: &str, ended: &str) -> Option<u64> {
    let a = chrono::DateTime::parse_from_rfc3339(started).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(ended).ok()?;
    u64::try_from((b - a).num_milliseconds()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 84, record 0097 (design D2, D4), the host-requirements spec's "A
    /// run an older sluice started": a run's request an older sluice wrote
    /// noted no system, and reads no switch whatever the link names now.
    #[test]
    fn a_run_noted_with_no_system_reads_no_switch() {
        let older = serde_json::json!({
            "runner": "moon",
            "revision": "0123456789abcdef0123456789abcdef01234567",
            "place": "/tmp/sluice/try/main/4",
            "targets": ["a:test"],
            "executor": {"kind": "sandbox", "confined": false},
            "host": "a-host",
            "started_ms": 1_780_000_000_000_i64,
        });
        let meta: Meta = serde_json::from_value(older).unwrap();
        assert_eq!(meta.nix_system, None);
        for now in [
            Some(Path::new("/nix/store/bbb-darwin-system")),
            Some(Path::new("/nix/store/aaa-darwin-system")),
            None,
        ] {
            assert_eq!(switch(meta.nix_system.as_deref(), now), None, "{now:?}");
        }
    }

    /// A host's paths for a sandboxed run, made under a directory of the
    /// test's own and reached through a link, so each given path differs
    /// from where it resolves, as `/tmp` is `/private/tmp`.
    struct Host {
        root: PathBuf,
        link: PathBuf,
    }

    impl Host {
        fn new(name: &str) -> Host {
            let root = std::env::temp_dir().join(format!("sluice-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("real")).unwrap();
            let link = root.join("link");
            std::os::unix::fs::symlink(root.join("real"), &link).unwrap();
            Host { root, link }
        }

        /// `rel` made under the host's real directory, named through the
        /// link.
        fn dir(&self, rel: &str) -> PathBuf {
            std::fs::create_dir_all(self.root.join("real").join(rel)).unwrap();
            self.link.join(rel)
        }

        fn place(&self, at: &str) -> Place {
            Place {
                name: "p".into(),
                path: self.dir(&format!("{at}/checkout")),
                tmp: self.dir(&format!("{at}/tmp")),
                env: BTreeMap::new(),
            }
        }

        /// `n` caches, each with a socket.
        fn caches(&self, n: usize) -> Vec<HostCache> {
            (0..n)
                .map(|i| {
                    let dir = self.dir(&format!("caches/{i}"));
                    HostCache {
                        socket: Some(dir.join("server.sock")),
                        dir,
                        env: BTreeMap::new(),
                    }
                })
                .collect()
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sandbox(network: Option<Network>, docker: bool) -> Limits {
        Limits {
            executor: Some("sandbox".into()),
            memory: None,
            network,
            docker,
        }
    }

    /// 83, 138, design D1 (the check-grant spec's "The grant is the
    /// profile the run writes"): the text a run's grant digests is the
    /// profile it writes with each path named by its role, so each role
    /// put back for its paths gives the written profile's rules.
    #[test]
    fn the_grant_renders_the_profile_the_run_writes() {
        let host = Host::new("grant-renders");
        let place = host.place("a");
        let caches = host.caches(2);
        let store = host.dir("home/.moon/cache/shared");
        let engines = vec![
            Endpoint::Unix(host.dir("engine").join("docker.sock")),
            Endpoint::Tcp {
                host: "localhost".into(),
                port: 2375,
                tls: None,
            },
            Endpoint::Tcp {
                host: "engine.example".into(),
                port: 2376,
                tls: None,
            },
        ];
        let reach = sandbox_reach(
            &place,
            &sandbox(Some(Network::Localhost), true),
            &[".last-used"],
            &caches,
            Some(&store),
            engines,
            Some(std::fs::canonicalize(host.dir("user-tmp")).unwrap()),
        );
        let written = seatbelt(&reach, false);
        let roles = seatbelt(&reach, true);
        for path in [&place.path, &place.tmp, &caches[0].dir, &store] {
            let real = std::fs::canonicalize(path).unwrap();
            assert!(written.contains(&*real.to_string_lossy()), "{written}");
            assert!(!roles.contains(&*real.to_string_lossy()), "{roles}");
        }

        let back = |role: Role| -> Vec<String> {
            match role {
                Role::UserTmp => reach
                    .user_tmp
                    .iter()
                    .map(|t| regex_quote(&t.to_string_lossy()))
                    .collect(),
                Role::EngineTcp => vec!["localhost:2375".into(), "*:2376".into()],
                _ => reach
                    .writable
                    .iter()
                    .chain(&reach.sockets)
                    .filter(|(r, _)| *r == role)
                    .map(|(_, p)| p.to_string_lossy().into_owned())
                    .collect(),
            }
        };
        let all = [
            Role::Place,
            Role::Scratch,
            Role::Cache,
            Role::CacheSocket,
            Role::EngineSocket,
            Role::EngineTcp,
            Role::UserTmp,
        ];
        let mut put_back = std::collections::BTreeSet::new();
        for line in roles.lines() {
            let named: Vec<Role> = all
                .into_iter()
                .filter(|r| line.contains(r.token()))
                .collect();
            match named.as_slice() {
                [] => {
                    put_back.insert(line.to_string());
                }
                [role] => {
                    let paths = back(*role);
                    assert!(!paths.is_empty(), "{role:?} names no path");
                    for p in paths {
                        put_back.insert(line.replace(role.token(), &p));
                    }
                }
                more => panic!("{line} names {more:?}"),
            }
        }
        let rules: std::collections::BTreeSet<String> =
            written.lines().map(str::to_string).collect();
        assert_eq!(put_back, rules);

        let profile = reach.profile();
        assert_eq!(profile.text, written);
        use sha2::{Digest, Sha256};
        let hex: String = Sha256::digest(roles.as_bytes())[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(profile.grant, format!("sb1:{hex}"));
        assert_eq!(profile.grant.len(), "sb1:".len() + 32);
    }

    /// 138, design D1 (the check-grant spec's "Two places, two hosts, one
    /// profile"): two checkouts, two scratch directories, two and three
    /// caches, each given and resolved, the runner's store under two homes
    /// outside the checkout, and two temporary directories give one grant;
    /// a store in the checkout gives the grant of a run given none.
    #[test]
    fn one_profile_gives_one_grant_wherever_it_runs() {
        let (one, two) = (Host::new("grant-one"), Host::new("grant-two"));
        let limits = sandbox(Some(Network::Localhost), false);
        let grant = |host: &Host, at: &str, caches: usize, store: Option<&Path>| {
            sandbox_reach(
                &host.place(at),
                &limits,
                &[".last-used"],
                &host.caches(caches),
                store,
                Vec::new(),
                Some(std::fs::canonicalize(host.dir(&format!("{at}/user-tmp"))).unwrap()),
            )
            .profile()
        };
        let a = grant(&one, "a", 2, Some(&one.dir("home-a/.moon/cache/shared")));
        let b = grant(&two, "b", 3, Some(&two.dir("b-home/cache/shared")));
        assert_ne!(a.text, b.text);
        assert_eq!(a.grant, b.grant);
        assert!(a.grant.starts_with("sb1:"), "{}", a.grant);

        let none = grant(&one, "c", 0, None);
        let inside = grant(&two, "d", 0, Some(&two.dir("d/checkout/.moon/cache")));
        assert_eq!(inside.grant, none.grant);
        assert_ne!(none.grant, a.grant);
    }

    /// 138, design D1 (the check-grant spec's "A profile that reaches
    /// more"): a run's network, whether it may reach the container engine,
    /// and whether it is given any cache each give another grant.
    #[test]
    fn a_profile_that_reaches_more_gives_another_grant() {
        let host = Host::new("grant-more");
        let place = host.place("a");
        let caches = host.caches(1);
        let engines = vec![Endpoint::Unix(host.dir("engine").join("docker.sock"))];
        let grant = |limits: Limits, caches: &[HostCache]| {
            sandbox_reach(&place, &limits, &[], caches, None, engines.clone(), None)
                .profile()
                .grant
        };
        let networks: std::collections::BTreeSet<String> =
            [Network::Denied, Network::Localhost, Network::Allowed]
                .into_iter()
                .map(|n| grant(sandbox(Some(n), false), &caches))
                .collect();
        assert_eq!(networks.len(), 3, "{networks:?}");
        assert_ne!(
            grant(sandbox(None, true), &caches),
            grant(sandbox(None, false), &caches)
        );
        assert_ne!(
            grant(sandbox(None, false), &caches),
            grant(sandbox(None, false), &[])
        );
    }

    /// 139, design D4 (the run-outcome spec's "Every refusal rule carries
    /// the mark"): under each network, every refusal rule of a run's
    /// profile carries the run's mark but the one for `/dev/dtracehelper`,
    /// which reports nothing; two marks give one grant.
    #[test]
    fn each_refusal_of_the_profile_carries_the_runs_mark() {
        let host = Host::new("grant-mark");
        let place = host.place("a");
        let mark = "sluice-run:0123456789abcdef";
        for network in [
            None,
            Some(Network::Denied),
            Some(Network::Localhost),
            Some(Network::Allowed),
        ] {
            let reach = Reach {
                mark: Some(mark.into()),
                ..sandbox_reach(
                    &place,
                    &sandbox(network, false),
                    &[],
                    &host.caches(1),
                    None,
                    Vec::new(),
                    None,
                )
            };
            let text = reach.profile().text;
            let denies: Vec<&str> = text.lines().filter(|l| l.starts_with("(deny")).collect();
            let want = match network {
                Some(Network::Denied | Network::Localhost) => 3,
                _ => 2,
            };
            assert_eq!(denies.len(), want, "{network:?}: {text}");
            for d in denies {
                if d.contains("/dev/dtracehelper") {
                    assert_eq!(
                        d,
                        "(deny file-write-data (literal \"/dev/dtracehelper\") (with no-report))"
                    );
                } else {
                    assert!(
                        d.ends_with(&format!("(with message \"{mark}\"))")),
                        "{network:?}: {d}"
                    );
                }
            }
            assert!(seatbelt(&reach, true).contains("(with message \"<mark>\")"));
            let other = Reach {
                mark: Some("sluice-run:fedcba9876543210".into()),
                ..reach.clone()
            };
            assert_ne!(other.profile().text, reach.profile().text);
            assert_eq!(other.profile().grant, reach.profile().grant);
        }
        let dir = Path::new("/tmp/sluice/.runs/p");
        assert_ne!(run_mark(dir, 1), run_mark(dir, 2));
        assert!(run_mark(dir, 1).starts_with("sluice-run:"));
        assert_eq!(run_mark(dir, 1).len(), "sluice-run:".len() + 16);
    }

    /// The host-caches spec's "Only the store is opened", "A store moon has
    /// not made yet" and "A store in the checkout" (C.2 sandbox row as
    /// amended at agentplot/blueprints 0c38f24; record 0105; rules core.9,
    /// core.35): a store outside the checkout is opened by its path and
    /// where it resolves, and neither moon's home nor a directory beside
    /// the store is; one not made yet under a link is opened where it will
    /// resolve, and nothing of it is made; one in the checkout or its
    /// scratch leaves the profile the one written with none.
    #[test]
    fn the_profile_opens_the_runners_store_outside_the_place() {
        let root = std::env::temp_dir().join(format!("sluice-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real = root.join("real");
        let (checkout, tmp, home) = (real.join("checkout"), real.join("tmp"), real.join("home"));
        for d in [
            &checkout,
            &tmp,
            &home.join("cache/shared"),
            &home.join("cache/beside"),
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        // The home reached through a link, as `/tmp` is `/private/tmp`.
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap();
        let place = Place {
            name: "p".into(),
            path: checkout.clone(),
            tmp: tmp.clone(),
            env: BTreeMap::new(),
        };
        let profile = |store: Option<&Path>| {
            sandbox_profile(
                &place,
                &Limits::default(),
                &[],
                &[],
                store,
                Vec::new(),
                None,
            )
            .text
        };
        let opened = |p: &str| -> std::collections::BTreeSet<PathBuf> {
            p.split("(subpath \"")
                .skip(1)
                .filter_map(|s| s.split("\")").next())
                .map(PathBuf::from)
                .collect()
        };
        let none = opened(&profile(None));

        // Only the store is opened, by its path and where it resolves.
        let store = link.join("home/cache/shared");
        let with = opened(&profile(Some(&store)));
        let added: Vec<_> = with.difference(&none).cloned().collect();
        let mut want = vec![store.clone(), canon(&real).join("home/cache/shared")];
        want.sort();
        assert_eq!(added, want);
        for not in [&home, &home.join("cache"), &home.join("cache/beside")] {
            for p in [not.clone(), canon(not)] {
                assert!(!with.contains(&p), "{} is opened", p.display());
            }
        }

        // A store moon has not made yet, under the link.
        let fresh = link.join("fresh/cache/shared");
        let with = opened(&profile(Some(&fresh)));
        let added: Vec<_> = with.difference(&none).cloned().collect();
        let mut want = vec![fresh.clone(), canon(&real).join("fresh/cache/shared")];
        want.sort();
        assert_eq!(added, want);
        assert!(!real.join("fresh").exists(), "sluice made the store");

        // A store in the checkout, or in its scratch, by any path, adds
        // nothing.
        for inside in [
            checkout.join(".moon/cache"),
            link.join("checkout/.moon/cache"),
            tmp.join("store"),
        ] {
            assert_eq!(
                profile(Some(&inside)),
                profile(None),
                "{}",
                inside.display()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
