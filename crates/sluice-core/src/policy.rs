//! The policy: `.config/sluice.toml`, read from the branch's current
//! revision (56, 27; ADR 0007; rule core.15).
//!
//! The core gives `[tests]` and `[checkout]` no meaning: they name the
//! profiles that bind the check and place seams and their settings, which
//! the core passes on (52; rule core.1). It reads the `[branches]` tables,
//! the gate of each branch, and `[executor]`, the limits a run is held to
//! (83). Two readers read only which branches are guarded and where
//! their records are, [`Policy::guarding`]: branch protection, and the
//! routing that decides which remote a branch is on, so a revision
//! refused elsewhere in its policy still says where its branch is (124;
//! ADRs 0059, 0069).

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sluice_seams::{Limits, Network, Setting, Settings};

/// Where the policy lives in a branch's tree.
pub const POLICY_PATH: &str = ".config/sluice.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// `[tests]`, for the check seam's profile.
    pub tests: Settings,
    /// `[checkout]`, for the place seam's profile.
    pub checkout: Settings,
    /// `[executor]`: what every run of the repository may use and reach.
    pub limits: Limits,
    /// Patterns of branch names and their gates, in the order written.
    pub branches: Vec<(String, Rules)>,
    /// `[landing] subject`: a landing commit's first line, with `{id}`,
    /// `{from}`, `{agent}` and `{to}` filled in (23). `{from}` is what
    /// lands: the submit's subject, or the branch submitted where it gave
    /// none.
    pub landing_subject: String,
    /// `[preview]`: how a preview of this revision is brought up
    /// (previews-1 clauses 98, 99); `None` when the table is absent.
    pub preview: Option<Preview>,
    /// `[systems]`: what a run may execute on, by name (85, 86).
    pub systems: BTreeMap<String, System>,
    /// `[signers]`: who may sign runs, and the most each may claim (78,
    /// 79). A policy with signers keeps every branch with a `remote` on
    /// its remote (C.6).
    pub signers: BTreeMap<String, Signer>,
    /// `[pools]`, in the order written: where the lead starts the runs no
    /// standing executor can take (88, 107).
    pub pools: Vec<Pool>,
    /// What the policy says that sluice reads differently than it is
    /// written, for `sluice status` to show.
    pub warnings: Vec<String>,
}

/// `[systems.<name>]`: a processor architecture and operating system, as
/// the toolchain names them (85).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    pub os: String,
    pub arch: String,
}

/// `[signers.<name>]` (78, 79): one identity, which the records seam's
/// profile reads and the core does not, and the claims it may make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signer {
    /// The identity's kind, the one key of the table that is not a claim,
    /// and its value.
    pub identity: (String, Setting),
    /// Whether its runs may be read as confined (29, 79).
    pub confined: bool,
    /// The systems its runs may claim; `None` for every system.
    pub systems: Option<Vec<String>>,
}

impl Signer {
    /// 79: whether a run it signed may claim `system`.
    pub fn may_claim(&self, system: &str) -> bool {
        self.systems
            .as_ref()
            .is_none_or(|s| s.iter().any(|x| x == system))
    }
}

/// `[pools.<name>]` (88, 107): a job service the check seam's launched
/// profile drives, the signer its jobs sign as, and its runner for each
/// system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub name: String,
    /// The pool's kind, the one key of the table that is not `signer` or
    /// `runners`, and its value, for the profile.
    pub kind: (String, Setting),
    pub signer: String,
    pub runners: BTreeMap<String, String>,
}

/// `[preview]`: what brings a preview of the revision up, and the
/// addresses a person opens (previews-1 clauses 98, 99).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The command, first word first: a list, or a string run by `sh -c`.
    pub command: Vec<String>,
    /// Each address by its label, a template over `{name.<port>}` and
    /// `{port.<port>}`; the place seam fills them in and refuses a port
    /// `[checkout] ports` does not list.
    pub addresses: BTreeMap<String, String>,
    /// How long the command may run; the branch's time limit when unset.
    pub timeout: Option<Duration>,
}

/// A conventional commit's subject by default.
pub const LANDING_SUBJECT: &str = "chore(land): #{id} {from}";

/// How a flaky key reads (19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flaky {
    Pass,
    Fail,
}

/// One branch's gate (27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    pub required: Vec<String>,
    /// ADR 0050: the line of `.config/sluice.toml` each of `required` is
    /// written on, in the same order, so a refusal can name it.
    pub required_lines: Vec<usize>,
    /// The window's upper bound; its lower bound is one (13).
    pub parallel: usize,
    /// Deepest first, or every candidate (14).
    pub batch: bool,
    pub flaky: Flaky,
    /// How long one candidate's checks may run (66).
    pub timeout: Option<Duration>,
    /// How long a passed candidate may wait to land while a deeper one
    /// holding it is under test (117). `None` holds nothing.
    pub hold: Option<Duration>,
    /// How many more times a checkout's release that failed is tried, one
    /// a round, before it is held (123).
    pub release_retries: u32,
    /// Which results satisfy the gate (27 to 29): on a branch whose
    /// records are on its remote, signer names (78); otherwise
    /// `confined` and executor kinds.
    pub trust: Vec<String>,
    /// Whether the branch's records are on its remote: it names a
    /// `remote`, and the policy names signers (C.6).
    pub signed: bool,
    /// Whether only confined runs count, on a branch whose records are
    /// on its remote (29, 79).
    pub confined: bool,
    /// The systems every required check must pass on; empty for any one
    /// system (86).
    pub systems: Vec<String>,
    /// The systems one check must pass on, in place of `systems` (86).
    pub systems_for: BTreeMap<String, Vec<String>>,
    /// On a branch whose records are on its remote, what each signer it
    /// trusts may claim (79): whether confined, and its systems.
    pub claims: BTreeMap<String, (bool, Option<Vec<String>>)>,
    /// The git remote the branch lives on (C.4, C.7): read by fetch and
    /// landed by a leased push. `None` for a local branch.
    pub remote: Option<String>,
    /// C.7 `pull_requests`: the branch takes its work as pull requests on
    /// its remote, submitted once these conditions hold. `None` for a
    /// branch whose work is submitted straight from a session's place.
    pub pull_requests: Option<PullRequests>,
    /// 137 (design D22): the branch takes pull requests and the policy
    /// names no `[signers]`, so its records are each host's own and no
    /// host keeps it on its remote to take them in: [`Policy::rules_for`]
    /// refuses this branch alone.
    pub unkept: bool,
}

/// 137 (design D22): a branch that takes pull requests on records each
/// host keeps, refused alone, naming the branch and its remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoIntake {
    pub branch: String,
    pub remote: String,
}

impl std::fmt::Display for NoIntake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (branch, remote) = (&self.branch, &self.remote);
        write!(
            f,
            "{branch} takes pull requests, which only the host that keeps it on {remote} takes \
             in, and its records are each host's own: name [signers] so they are kept on \
             {remote} (137)"
        )
    }
}

impl std::error::Error for NoIntake {}

/// C.7 `pull_requests`: what a pull request into the branch meets before
/// its driver submits it (27). Read from the branch's revision (56), so a
/// pull request cannot loosen its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PullRequests {
    /// Approving reviews on the pull request's current head.
    pub reviews: u32,
    /// Checks other systems post on its head, by name, each to have passed.
    pub checks: Vec<String>,
    /// ADR 0044: how many of the last lines of a failed check's output a
    /// pull request's report posts, scrubbed; `None` posts none, only
    /// where the output is kept.
    pub log_lines: Option<usize>,
}

/// What `trust` may list on a branch whose records are this host's.
const TRUSTS: [&str; 4] = ["confined", "open", "sandbox", "container"];

impl Rules {
    /// 28: whether a ran result's executor satisfies the gate.
    pub fn trusts_run(&self, kind: &str, confined: bool) -> bool {
        self.trust.iter().any(|t| t == kind) || (confined && self.trusts("confined"))
    }

    /// 78, 79: whether a result signed by `signer`, claiming `confined`
    /// on `system`, counts for a branch whose records are on its remote:
    /// the branch trusts the signer, and every claim it relies on is one
    /// the signer may make.
    pub fn trusts_signed(&self, signer: &str, confined: bool, system: Option<&str>) -> bool {
        let Some((may_confine, systems)) = self.claims.get(signer) else {
            return false;
        };
        if self.confined && !(confined && *may_confine) {
            return false;
        }
        match (system, systems) {
            (Some(s), Some(list)) => list.iter().any(|x| x == s),
            _ => true,
        }
    }

    /// Whether `trust` lists `what`.
    pub fn trusts(&self, what: &str) -> bool {
        self.trust.iter().any(|t| t == what)
    }

    /// 29: whether a run of the executor `kind` can ever satisfy the gate:
    /// only a container is confined today. On a branch whose records are
    /// on its remote, what satisfies the gate is a signer's (78), so no
    /// executor kind rules a run out.
    pub fn satisfiable_by(&self, kind: &str) -> bool {
        self.signed || self.trusts(kind) || (kind == "container" && self.trusts("confined"))
    }

    /// 86: the systems a pass of `check` must come from; empty for any
    /// one system.
    pub fn systems_of(&self, check: &str) -> &[String] {
        self.systems_for.get(check).unwrap_or(&self.systems)
    }
}

impl Default for Rules {
    /// What a table leaves out: every candidate, a window of one to four,
    /// a flaky key read as failed, an hour per candidate, and every run
    /// sluice starts trusted, no replay (ADR 0015).
    fn default() -> Self {
        Rules {
            required: Vec::new(),
            required_lines: Vec::new(),
            parallel: 4,
            batch: false,
            flaky: Flaky::Fail,
            timeout: Some(Duration::from_secs(3600)),
            hold: None,
            release_retries: crate::step::RELEASE_RETRIES,
            trust: vec!["open".into(), "sandbox".into(), "container".into()],
            signed: false,
            confined: false,
            systems: Vec::new(),
            systems_for: BTreeMap::new(),
            claims: BTreeMap::new(),
            remote: None,
            pull_requests: None,
            unkept: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileForm {
    #[serde(default)]
    tests: BTreeMap<String, toml::Value>,
    #[serde(default)]
    checkout: BTreeMap<String, toml::Value>,
    #[serde(default)]
    executor: ExecutorForm,
    /// Each key with where it is written, so the tables keep the order
    /// the file gives them.
    #[serde(default)]
    branches: BTreeMap<toml::Spanned<String>, RulesForm>,
    #[serde(default)]
    landing: LandingForm,
    preview: Option<PreviewForm>,
    #[serde(default)]
    systems: BTreeMap<String, SystemForm>,
    #[serde(default)]
    signers: BTreeMap<String, BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pools: BTreeMap<String, toml::Spanned<BTreeMap<String, toml::Value>>>,
}

/// What [`Policy::guarding`] reads, every other table passed over.
#[derive(Deserialize)]
struct GuardingForm {
    /// Each key with where it is written, as `FileForm` keeps them.
    #[serde(default)]
    branches: BTreeMap<toml::Spanned<String>, toml::Value>,
    signers: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemForm {
    os: String,
    arch: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewForm {
    command: Option<toml::Value>,
    #[serde(default)]
    addresses: BTreeMap<String, String>,
    timeout: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct LandingForm {
    subject: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ExecutorForm {
    kind: Option<String>,
    memory: Option<String>,
    network: Option<Network>,
    docker: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesForm {
    /// Each entry with where it is written, so a refusal can name its
    /// line (ADR 0050).
    #[serde(default)]
    required: Vec<toml::Spanned<String>>,
    parallel: Option<i64>,
    batch: Option<bool>,
    flaky: Option<String>,
    timeout: Option<String>,
    hold: Option<String>,
    release_retries: Option<i64>,
    trust: Option<Vec<String>>,
    remote: Option<String>,
    pull_requests: Option<PullRequestsForm>,
    confined: Option<bool>,
    systems: Option<Vec<String>>,
    systems_for: Option<BTreeMap<String, Vec<String>>>,
    // The host's, never a branch's (74): read only to refuse them by name.
    lease: Option<toml::Value>,
    alive: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PullRequestsForm {
    reviews: Option<i64>,
    #[serde(default)]
    checks: Vec<String>,
    log_lines: Option<i64>,
}

impl Policy {
    pub fn parse(text: &str) -> Result<Policy> {
        let form: FileForm = toml::from_str(text).context("reading .config/sluice.toml")?;
        let tests = settings(form.tests, "[tests]")?;
        // 35, 71; ADR 0049: the checkout cache was the statement's, not a
        // profile's, and the statement no longer has it, so the core reads
        // this one word of `[checkout]`: a policy that still declares it is
        // refused, saying what replaced it, so no repository's checks start
        // cold with no word to anyone.
        if form.checkout.contains_key("cache") {
            bail!(
                "[checkout] declares a cache, which sluice no longer keeps: a checkout starts \
                 with nothing but its revision, and the caches its tests share are the host's \
                 [[caches]], which the host's keeper declares in ~/.config/sluice/config.toml \
                 (ADR 0049). Remove [[checkout.cache]] from .config/sluice.toml"
            );
        }
        let checkout = settings(form.checkout, "[checkout]")?;
        let limits = Limits {
            executor: form.executor.kind,
            memory: match form.executor.memory.as_deref() {
                None => None,
                Some(m) => Some(parse_size(m).context("[executor] memory")?),
            },
            network: form.executor.network,
            docker: form.executor.docker.unwrap_or(false),
        };
        let systems: BTreeMap<String, System> = form
            .systems
            .into_iter()
            .map(|(n, s)| {
                (
                    n,
                    System {
                        os: s.os,
                        arch: s.arch,
                    },
                )
            })
            .collect();
        let known = |what: &str, names: &[String]| -> Result<()> {
            match names.iter().find(|n| !systems.contains_key(*n)) {
                Some(n) => bail!(
                    "{what} names the system {n:?}, which [systems] does not name{}",
                    listed(systems.keys())
                ),
                None => Ok(()),
            }
        };
        let mut signers = BTreeMap::new();
        for (name, mut table) in form.signers {
            let at = format!("[signers.{name}]");
            let confined = match table.remove("confined") {
                None => false,
                Some(toml::Value::Boolean(b)) => b,
                Some(v) => bail!("{at} confined must be true or false, not {v}"),
            };
            let systems = match table.remove("systems") {
                None => None,
                Some(v) => Some(strings(v, &format!("{at} systems"))?),
            };
            if let Some(s) = &systems {
                known(&at, s)?;
            }
            let identity = one(table, &at, "identity")?;
            signers.insert(
                name,
                Signer {
                    identity,
                    confined,
                    systems,
                },
            );
        }
        let mut pools: Vec<(usize, Pool)> = Vec::new();
        for (name, spanned) in form.pools {
            let at = format!("[pools.{name}]");
            let start = spanned.span().start;
            let mut table = spanned.into_inner();
            let signer = match table.remove("signer") {
                Some(toml::Value::String(s)) => s,
                Some(v) => bail!("{at} signer must name a signer, not {v}"),
                None => bail!("{at} names no signer; its jobs sign as one of [signers]"),
            };
            let runners = match table.remove("runners") {
                Some(toml::Value::Table(t)) if !t.is_empty() => t
                    .into_iter()
                    .map(|(s, v)| match v {
                        toml::Value::String(l) => Ok((s, l)),
                        v => bail!("{at} runners.{s} must be a string, not {v}"),
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?,
                _ => bail!(
                    "{at} names no runners: a table of each system it runs and the runner for it"
                ),
            };
            let kind = one(table, &at, "kind")?;
            let Some(s) = signers.get(&signer) else {
                bail!("{at} signs as {signer:?}, which [signers] does not name");
            };
            if s.identity.0 != kind.0 {
                bail!(
                    "{at} is a {} pool and its signer {signer} signs by {}; a pool's jobs sign as a signer of its own kind",
                    kind.0,
                    s.identity.0
                );
            }
            let names: Vec<String> = runners.keys().cloned().collect();
            known(&format!("{at} runners"), &names)?;
            if let Some(n) = names.iter().find(|n| !s.may_claim(n)) {
                bail!("{at} has a runner for {n}, which its signer {signer} may not claim (79)");
            }
            pools.push((
                start,
                Pool {
                    name,
                    kind,
                    signer,
                    runners,
                },
            ));
        }
        pools.sort_by_key(|(start, _)| *start);
        let pools: Vec<Pool> = pools.into_iter().map(|(_, p)| p).collect();
        let mut written: Vec<_> = form.branches.into_iter().collect();
        written.sort_by_key(|(p, _)| p.span().start);
        let mut branches = Vec::new();
        let mut warnings = Vec::new();
        for (pattern, r) in written {
            let pattern = pattern.into_inner();
            let d = Rules::default();
            let at = format!("[branches.\"{pattern}\"]");
            for (key, v) in [("lease", &r.lease), ("alive", &r.alive)] {
                if v.is_some() {
                    bail!(
                        "{at} sets {key}, which is the host's, never a branch's (74): \
                         set it under [peers] in the host's config"
                    );
                }
            }
            let signed = r.remote.is_some() && !signers.is_empty();
            let parallel = match r.parallel {
                None => d.parallel,
                Some(n) if n >= 1 => n as usize,
                Some(n) => bail!("[branches.\"{pattern}\"] parallel must be at least 1, not {n}"),
            };
            let flaky = match r.flaky.as_deref() {
                None => d.flaky,
                Some("pass") => Flaky::Pass,
                Some("fail") => Flaky::Fail,
                Some(other) => {
                    bail!("[branches.\"{pattern}\"] flaky must be \"pass\" or \"fail\", not {other:?}")
                }
            };
            let timeout = match r.timeout.as_deref() {
                None => d.timeout,
                Some(t) => Some(
                    parse_duration(t)
                        .with_context(|| format!("[branches.\"{pattern}\"] timeout"))?,
                ),
            };
            let hold = match r.hold.as_deref() {
                None => d.hold,
                Some(t) => Some(parse_duration(t).with_context(|| format!("{at} hold"))?)
                    .filter(|h| !h.is_zero()),
            };
            let release_retries = match r.release_retries {
                None => d.release_retries,
                Some(n) if (0..=u32::MAX as i64).contains(&n) => n as u32,
                Some(n) => bail!("{at} release_retries must be at least 0, not {n}"),
            };
            let mut trust = match r.trust {
                Some(t) => t,
                None if signed => Vec::new(),
                None => d.trust,
            };
            // 108: the cache supplies outputs and decides nothing; refusing
            // the word would refuse the policy a branch already holds
            // (core.15), so it is read as absent.
            if trust.iter().any(|t| t == "cache") {
                trust.retain(|t| t != "cache");
                warnings.push(format!(
                    "{at} trust lists \"cache\", which no longer counts: a replay counts \
                     only through a pass recorded for its key (108)"
                ));
            }
            if signed {
                if let Some(t) = trust.iter().find(|t| !signers.contains_key(*t)) {
                    let what = if TRUSTS.contains(&t.as_str()) {
                        "an executor kind"
                    } else {
                        "no signer"
                    };
                    bail!(
                        "{at} trust lists {t:?}, {what}; on a branch whose state is on its \
                         remote, trust lists signers{}",
                        listed(signers.keys())
                    );
                }
            } else if let Some(t) = trust.iter().find(|t| !TRUSTS.contains(&t.as_str())) {
                bail!("{at} trust lists {t:?}; it may list {}", TRUSTS.join(", "));
            }
            let confined = match r.confined {
                Some(true) if !signed => bail!(
                    "{at} sets confined, which asks a branch whose state is on its remote for \
                     confined runs; this branch keeps its state on this host, so list \
                     \"confined\" in trust"
                ),
                c => c.unwrap_or(false),
            };
            let systems_all = r.systems.unwrap_or_default();
            known(&format!("{at} systems"), &systems_all)?;
            let systems_for = r.systems_for.unwrap_or_default();
            for (check, s) in &systems_for {
                known(&format!("{at} systems_for.{check:?}"), s)?;
            }
            let pull_requests = match r.pull_requests {
                None => None,
                Some(_) if r.remote.is_none() => bail!(
                    "[branches.\"{pattern}\".pull_requests] needs remote: pull requests \
                     are on the remote the branch lives on, and {pattern} names none"
                ),
                Some(p) => Some(PullRequests {
                    reviews: match p.reviews {
                        None => 0,
                        Some(n) if n >= 0 => n as u32,
                        Some(n) => bail!(
                            "[branches.\"{pattern}\".pull_requests] reviews must be at least 0, not {n}"
                        ),
                    },
                    checks: p.checks,
                    log_lines: match p.log_lines {
                        None => None,
                        Some(n) if n >= 1 => Some(n as usize),
                        Some(n) => bail!(
                            "[branches.\"{pattern}\".pull_requests] log_lines must be at least 1, not {n}: \
                             leave it out to post no output"
                        ),
                    },
                }),
            };
            let claims = if signed {
                trust
                    .iter()
                    .map(|n| {
                        let s = &signers[n];
                        (n.clone(), (s.confined, s.systems.clone()))
                    })
                    .collect()
            } else {
                BTreeMap::new()
            };
            let required_lines = r
                .required
                .iter()
                .map(|e| line_at(text, e.span().start))
                .collect();
            branches.push((
                pattern,
                Rules {
                    required: r.required.into_iter().map(|e| e.into_inner()).collect(),
                    required_lines,
                    parallel,
                    batch: r.batch.unwrap_or(d.batch),
                    flaky,
                    timeout,
                    hold,
                    release_retries,
                    trust,
                    signed,
                    confined,
                    systems: systems_all,
                    systems_for,
                    claims,
                    remote: r.remote,
                    unkept: pull_requests.is_some() && signers.is_empty(),
                    pull_requests,
                },
            ));
        }
        let landing_subject = form
            .landing
            .subject
            .unwrap_or_else(|| LANDING_SUBJECT.into());
        let mut rest = landing_subject.as_str();
        while let Some(i) = rest.find('{') {
            let close = rest[i..]
                .find('}')
                .ok_or_else(|| anyhow!("[landing] subject {landing_subject:?} leaves a {{ open"))?;
            let name = &rest[i + 1..i + close];
            if !["id", "from", "agent", "to"].contains(&name) {
                bail!("[landing] subject names {{{name}}}; it may name {{id}}, {{from}}, {{agent}} and {{to}}");
            }
            rest = &rest[i + close + 1..];
        }
        if landing_subject.trim().is_empty() || landing_subject.contains('\n') {
            bail!("[landing] subject must be one line");
        }
        let preview = form.preview.map(preview).transpose()?;
        Ok(Policy {
            tests,
            checkout,
            limits,
            branches,
            landing_subject,
            preview,
            systems,
            signers,
            pools,
            warnings,
        })
    }

    /// 124: the policy only as far as guarding needs, which is as far as
    /// deciding which remote a branch is on needs: each `[branches]`
    /// pattern in the order written, with the default gate and its
    /// `remote`, and the names `[signers]` gives, so that
    /// `signers.is_empty()` says what it says in [`Policy::parse`] (C.6).
    /// Every other table and field, known or not, is passed over, and the
    /// rest of the policy is its defaults. It has two callers, branch
    /// protection (ADR 0059) and the routing of a branch to its remote
    /// (ADR 0069); the policy is read whole, with [`Policy::parse`], at
    /// the revision a branch's gate is read from (56).
    pub fn guarding(text: &str) -> Result<Policy> {
        let form: GuardingForm = toml::from_str(text).context("reading .config/sluice.toml")?;
        let mut written: Vec<_> = form.branches.into_iter().collect();
        written.sort_by_key(|(p, _)| p.span().start);
        let mut branches = Vec::new();
        for (pattern, table) in written {
            let pattern = pattern.into_inner();
            let at = format!("[branches.\"{pattern}\"]");
            let toml::Value::Table(mut table) = table else {
                bail!("{at} must be a table, not {}", table.type_str());
            };
            let remote = match table.remove("remote") {
                None => None,
                Some(toml::Value::String(r)) => Some(r),
                Some(v) => bail!("{at} remote must be the name of a git remote, not {v}"),
            };
            branches.push((
                pattern,
                Rules {
                    remote,
                    ..Rules::default()
                },
            ));
        }
        let signers = match form.signers {
            None => BTreeMap::new(),
            // Its identity and claims passed over: guarding verifies no run.
            Some(toml::Value::Table(t)) => t
                .into_iter()
                .map(|(name, _)| {
                    let signer = Signer {
                        identity: (String::new(), Setting::List(Vec::new())),
                        confined: false,
                        systems: None,
                    };
                    (name, signer)
                })
                .collect(),
            Some(v) => bail!("[signers] must be a table of signers, not {}", v.type_str()),
        };
        Ok(Policy {
            tests: Settings::new(),
            checkout: Settings::new(),
            limits: Limits::default(),
            branches,
            landing_subject: LANDING_SUBJECT.into(),
            preview: None,
            systems: BTreeMap::new(),
            signers,
            pools: Vec::new(),
            warnings: Vec::new(),
        })
    }

    /// The gate for `branch`: an exact name before any pattern, then the
    /// pattern with the most literal characters. `None` when no table
    /// names the branch, which is then not sluice's to land on.
    /// A branch that takes pull requests on records each host keeps is
    /// refused alone, with [`NoIntake`]'s words (137; design D22).
    pub fn rules_for(&self, branch: &str) -> Result<Option<&Rules>> {
        let Some(i) = self.table_for(branch)? else {
            return Ok(None);
        };
        let rules = &self.branches[i].1;
        if rules.unkept {
            return Err(NoIntake {
                branch: branch.to_string(),
                remote: rules.remote.clone().unwrap_or_default(),
            }
            .into());
        }
        Ok(Some(rules))
    }

    /// The remote the table that gives `branch` its gate names, whatever
    /// [`Policy::rules_for`] refuses of the table: what routing reads
    /// (core.27; design D22).
    pub fn remote_of(&self, branch: &str) -> Result<Option<String>> {
        Ok(self
            .table_for(branch)?
            .and_then(|i| self.branches[i].1.remote.clone()))
    }

    /// Which of the `[branches]` tables, in the order written, gives
    /// `branch` its gate, as `rules_for` chooses it.
    pub fn table_for(&self, branch: &str) -> Result<Option<usize>> {
        if let Some(i) = self.branches.iter().position(|(p, _)| p == branch) {
            return Ok(Some(i));
        }
        let mut best: Option<(usize, usize)> = None;
        let mut tied = false;
        for (i, (p, _)) in self.branches.iter().enumerate() {
            if !p.contains('*') || !glob(p, branch) {
                continue;
            }
            let weight = p.chars().filter(|c| *c != '*').count();
            match best {
                Some((w, _)) if w > weight => {}
                Some((w, _)) if w == weight => tied = true,
                _ => {
                    best = Some((weight, i));
                    tied = false;
                }
            }
        }
        if tied {
            let (_, i) = best.expect("a tie has a best");
            let p = &self.branches[i].0;
            bail!("{branch} matches more than one [branches] pattern as closely as {p:?}");
        }
        Ok(best.map(|(_, i)| i))
    }
}

/// `[preview]`, its command and the shape of its addresses checked.
fn preview(form: PreviewForm) -> Result<Preview> {
    let command = match form.command {
        None => bail!("[preview] names no command"),
        Some(toml::Value::String(s)) if !s.trim().is_empty() => {
            vec!["/bin/sh".into(), "-c".into(), s]
        }
        Some(toml::Value::Array(xs)) if !xs.is_empty() => xs
            .into_iter()
            .map(|x| match x {
                toml::Value::String(s) => Ok(s),
                other => bail!("[preview] command must list strings, not {other}"),
            })
            .collect::<Result<_>>()?,
        Some(_) => bail!("[preview] command must be a string or a list, the command first"),
    };
    for (label, template) in &form.addresses {
        let mut rest = template.as_str();
        while let Some(i) = rest.find('{') {
            let close = rest[i..].find('}').ok_or_else(|| {
                anyhow!("[preview] addresses {label} = {template:?} leaves a {{ open")
            })?;
            let inner = &rest[i + 1..i + close];
            match inner.split_once('.') {
                Some(("name" | "port", port)) if !port.is_empty() => {}
                _ => bail!(
                    "[preview] addresses {label} names {{{inner}}}; it may name {{name.<port>}} and {{port.<port>}}"
                ),
            }
            rest = &rest[i + close + 1..];
        }
    }
    let timeout = match form.timeout.as_deref() {
        None => None,
        Some(t) => Some(parse_duration(t).context("[preview] timeout")?),
    };
    Ok(Preview {
        command,
        addresses: form.addresses,
        timeout,
    })
}

/// A profile's table: each value a string, a number, a list of strings,
/// or an array of tables whose values are strings or lists of strings.
fn settings(table: BTreeMap<String, toml::Value>, section: &str) -> Result<Settings> {
    let mut out = Settings::new();
    for (k, v) in table {
        let setting = match v {
            toml::Value::String(s) => Setting::Text(s),
            // A number is its digits; the profile that reads it parses them.
            toml::Value::Integer(n) => Setting::Text(n.to_string()),
            toml::Value::Array(xs) if !xs.is_empty() && xs.iter().all(|x| x.is_table()) => {
                Setting::Tables(
                    xs.into_iter()
                        .map(|x| {
                            let toml::Value::Table(t) = x else {
                                unreachable!("every entry is a table")
                            };
                            let t: BTreeMap<String, toml::Value> = t.into_iter().collect();
                            if let Some((n, v)) = t.iter().find(|(_, v)| v.is_integer()) {
                                bail!("{section} [[{k}]] {n} must be a string or a list of strings, not {v}");
                            }
                            let inner = settings(t, &format!("{section} [[{k}]]"))?;
                            if inner.values().any(|s| matches!(s, Setting::Tables(_))) {
                                bail!("{section} [[{k}]] holds strings and lists of strings only");
                            }
                            Ok(inner)
                        })
                        .collect::<Result<_>>()?,
                )
            }
            toml::Value::Array(xs) => Setting::List(
                xs.into_iter()
                    .map(|x| match x {
                        toml::Value::String(s) => Ok(s),
                        // A number is its digits, as on its own.
                        toml::Value::Integer(n) => Ok(n.to_string()),
                        other => bail!("{section} {k} must list strings, not {other}"),
                    })
                    .collect::<Result<_>>()?,
            ),
            other => bail!(
                "{section} {k} must be a string, a number, a list of strings or an array of tables, not {other}"
            ),
        };
        out.insert(k, setting);
    }
    Ok(out)
}

/// A list of strings.
fn strings(v: toml::Value, at: &str) -> Result<Vec<String>> {
    match v {
        toml::Value::Array(xs) => xs
            .into_iter()
            .map(|x| match x {
                toml::Value::String(s) => Ok(s),
                other => bail!("{at} must list strings, not {other}"),
            })
            .collect(),
        other => bail!("{at} must be a list of strings, not {other}"),
    }
}

/// The one key left in a signer's or a pool's table, for its profile: a
/// string, a list of strings or a table of them.
fn one(table: BTreeMap<String, toml::Value>, at: &str, what: &str) -> Result<(String, Setting)> {
    let mut keys = table.into_iter();
    let Some((k, v)) = keys.next() else {
        bail!("{at} names no {what}");
    };
    if let Some((other, _)) = keys.next() {
        bail!("{at} names more than one {what}: {k} and {other}; it names one");
    }
    let setting = match v {
        toml::Value::String(s) => Setting::Text(s),
        toml::Value::Table(t) => Setting::Tables(vec![settings(
            t.into_iter().collect(),
            &format!("{at} {k}"),
        )?]),
        v @ toml::Value::Array(_) => Setting::List(strings(v, &format!("{at} {k}"))?),
        other => bail!("{at} {k} must be a string, a list of strings or a table, not {other}"),
    };
    Ok((k, setting))
}

/// `; [systems] names a, b`, or nothing when there are none.
fn listed<'a>(names: impl Iterator<Item = &'a String>) -> String {
    let names: Vec<&str> = names.map(String::as_str).collect();
    if names.is_empty() {
        String::new()
    } else {
        format!(": {}", names.join(", "))
    }
}

/// The line, counted from one, that byte `at` of `text` is on.
fn line_at(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].matches('\n').count() + 1
}

/// `"512M"`, `"8G"`: bytes, in powers of 1024.
pub fn parse_size(text: &str) -> Result<u64> {
    let t = text.trim();
    let (digits, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let n: u64 = digits
        .parse()
        .map_err(|_| anyhow!("{text:?} is not a size such as \"8G\""))?;
    let scale: u64 = match unit {
        "K" => 1 << 10,
        "M" => 1 << 20,
        "G" => 1 << 30,
        "T" => 1 << 40,
        _ => bail!("{text:?} is not a size such as \"8G\""),
    };
    n.checked_mul(scale)
        .ok_or_else(|| anyhow!("{text:?} is too large"))
}

/// `*` matches any run of characters but `/`.
fn glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[u8], n: &[u8]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => {
                let mut i = 0;
                loop {
                    if go(rest, &n[i..]) {
                        return true;
                    }
                    if i == n.len() || n[i] == b'/' {
                        return false;
                    }
                    i += 1;
                }
            }
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

/// `"20m"`, `"90s"`, `"1h30m"`.
pub fn parse_duration(text: &str) -> Result<Duration> {
    let mut total = 0u64;
    let mut digits = String::new();
    let mut any = false;
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n: u64 = digits
            .parse()
            .map_err(|_| anyhow!("{text:?} is not a duration such as \"20m\""))?;
        digits.clear();
        total += n * match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => bail!("{text:?} is not a duration such as \"20m\""),
        };
        any = true;
    }
    if !digits.is_empty() || !any {
        bail!("{text:?} is not a duration such as \"20m\"");
    }
    Ok(Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r##"
[tests]
runner = "script"
command = "tests/run"

[checkout]
method = "scratch"

[branches."bolt/*"]
required = ["#gate"]
parallel = 4
batch = false
flaky = "fail"
timeout = "20m"

[branches.main]
required = ["#gate", "#live"]
parallel = 2
batch = true
hold = "5m"
"##;

    #[test]
    fn reads_the_walkthrough_shape() {
        let p = Policy::parse(POLICY).unwrap();
        assert_eq!(p.tests["runner"], Setting::Text("script".into()));
        assert_eq!(p.tests["command"], Setting::Text("tests/run".into()));
        assert_eq!(p.checkout["method"], Setting::Text("scratch".into()));
        assert_eq!(p.limits, Limits::default());
        let bolt = p.rules_for("bolt/concurrent-units").unwrap().unwrap();
        assert_eq!(bolt.required, vec!["#gate"]);
        assert_eq!(bolt.parallel, 4);
        assert!(!bolt.batch);
        assert_eq!(bolt.flaky, Flaky::Fail);
        assert_eq!(bolt.timeout, Some(Duration::from_secs(1200)));
        let main = p.rules_for("main").unwrap().unwrap();
        assert!(main.batch);
        assert_eq!(main.parallel, 2);
        assert_eq!(main.flaky, Flaky::Fail);
        assert_eq!(main.timeout, Some(Duration::from_secs(3600)));
        assert_eq!(main.hold, Some(Duration::from_secs(300)));
        assert_eq!(bolt.hold, None);
        assert_eq!(bolt.release_retries, 3);
        assert!(p.rules_for("unit/x").unwrap().is_none());
        assert!(p.rules_for("bolt/a/b").unwrap().is_none());
        assert_eq!(main.trust, ["open", "sandbox", "container"]);
    }

    /// ADR 0050: each required entry keeps the line it is written on.
    #[test]
    fn keeps_the_line_of_a_required_entry_on_one_line() {
        let p = Policy::parse(POLICY).unwrap();
        let bolt = p.rules_for("bolt/x").unwrap().unwrap();
        assert_eq!(bolt.required_lines, [10]);
        let main = p.rules_for("main").unwrap().unwrap();
        assert_eq!(main.required_lines, [17, 17]);
    }

    /// ADR 0050: a `required` array written across lines keeps each
    /// entry's own line, as switchboard-kit writes it.
    #[test]
    fn keeps_the_line_of_each_entry_of_a_required_array_across_lines() {
        let p = Policy::parse(
            "[tests]\nrunner = \"moon\"\n\n\
             [branches.main]\n\
             required = [\n\
             \x20 \"#check-fast\",\n\
             \x20 \"root:integration\",\n\
             ]\n",
        )
        .unwrap();
        let main = p.rules_for("main").unwrap().unwrap();
        assert_eq!(main.required, ["#check-fast", "root:integration"]);
        assert_eq!(main.required_lines, [6, 7]);
    }

    /// The branch patterns that name no check under the moon runner: a
    /// branch with no `required` plans no task, so every submission to it
    /// lands untested.
    fn unchecked_under_moon(p: &Policy) -> Vec<&str> {
        if p.tests.get("runner") != Some(&Setting::Text("moon".into())) {
            return Vec::new();
        }
        p.branches
            .iter()
            .filter(|(_, rules)| rules.required.is_empty())
            .map(|(pattern, _)| pattern.as_str())
            .collect()
    }

    #[test]
    fn a_moon_branch_that_requires_nothing_is_unchecked() {
        let p = Policy::parse(
            "[tests]\nrunner = \"moon\"\n\n\
             [branches.\"dev/*\"]\nparallel = 2\n\n\
             [branches.main]\nrequired = [\"#test\"]\n",
        )
        .unwrap();
        assert_eq!(unchecked_under_moon(&p), ["dev/*"]);
    }

    const DIR: &str = "../../plugins/sluice/skills/setting-up-sluice/references/examples";

    /// The Claude Code plugin's example policies, each with a branch its
    /// comments say it gates.
    const EXAMPLES: [(&str, &str, &str); 4] = [
        (
            "moon-minimal.sluice.toml",
            include_str!(
                "../../../plugins/sluice/skills/setting-up-sluice/references/examples/moon-minimal.sluice.toml"
            ),
            "main",
        ),
        (
            "moon-worktrunk-github.sluice.toml",
            include_str!(
                "../../../plugins/sluice/skills/setting-up-sluice/references/examples/moon-worktrunk-github.sluice.toml"
            ),
            "main",
        ),
        (
            "command-only.sluice.toml",
            include_str!(
                "../../../plugins/sluice/skills/setting-up-sluice/references/examples/command-only.sluice.toml"
            ),
            "main",
        ),
        (
            "moon-peers-github.sluice.toml",
            include_str!(
                "../../../plugins/sluice/skills/setting-up-sluice/references/examples/moon-peers-github.sluice.toml"
            ),
            "main",
        ),
    ];

    /// The Claude Code plugin's example policies read, gate the branches
    /// their comments say, and hold every branch under moon to a check.
    #[test]
    fn the_plugins_example_policies_read() {
        let examples = EXAMPLES;
        for (name, text, branch) in examples {
            let p = Policy::parse(text).unwrap_or_else(|e| panic!("{DIR}/{name}: {e:#}"));
            assert!(
                p.rules_for(branch).unwrap().is_some(),
                "{DIR}/{name}: {branch}"
            );
            let unchecked = unchecked_under_moon(&p);
            assert!(
                unchecked.is_empty(),
                "{DIR}/{name}: {unchecked:?} name no check, so they land untested"
            );
        }
        let full = Policy::parse(examples[1].1).unwrap();
        assert!(full.rules_for("feature/x").unwrap().is_some());
        let main = full.rules_for("main").unwrap().unwrap();
        assert_eq!(main.remote.as_deref(), Some("origin"));
        assert!(main.pull_requests.is_some());
        // 137 (design D22): kept on origin, so the host that keeps it
        // takes its pull requests in.
        assert!(main.signed && !main.unkept);
        let peers = Policy::parse(examples[3].1).unwrap();
        let main = peers.rules_for("main").unwrap().unwrap();
        assert!(main.signed && main.confined);
        assert_eq!(main.systems_of("cli:build").len(), 3);
    }

    /// sluice's own policy reads, and gates as section 9 says: bolt
    /// branches on the fast tier from any run or the cache, `main` on
    /// every tier from sandboxed or confined runs only, deepest first,
    /// landed on `origin`, taking its work as pull requests that wait on
    /// nothing beyond the gate.
    #[test]
    fn sluices_own_policy_reads() {
        let p = Policy::parse(include_str!("../../../.config/sluice.toml")).unwrap();
        assert_eq!(p.limits.executor.as_deref(), Some("sandbox"));
        assert_eq!(p.limits.network, Some(Network::Localhost));
        assert!(p.checkout.is_empty(), "{:?}", p.checkout);
        let bolt = p.rules_for("bolt/x").unwrap().unwrap();
        assert_eq!(bolt.required, ["#check-fast"]);
        assert!(bolt.satisfiable_by("sandbox") && !bolt.trusts("cache"));
        assert_eq!(bolt.remote.as_deref(), Some("origin"));
        assert_eq!(bolt.pull_requests, None);
        // 137 (design D22): main takes pull requests and the policy names
        // no [signers], so main is refused alone until it names them, which
        // is ops' to choose; its remote still reads for routing.
        match p.rules_for("main") {
            Ok(Some(main)) => {
                assert_eq!(main.required, ["#check-fast", "#check-tools", "#check-e2e"]);
                assert!(main.batch && main.satisfiable_by("sandbox") && !main.trusts("cache"));
                assert_eq!(main.remote.as_deref(), Some("origin"));
                assert_eq!(main.pull_requests, Some(PullRequests::default()));
                assert!(main.signed, "main names [signers] once it reads");
            }
            Ok(None) => panic!("main has no table"),
            Err(e) => {
                assert!(e.downcast_ref::<NoIntake>().is_some(), "{e:#}");
                assert!(format!("{e:#}").contains("[signers]"), "{e:#}");
            }
        }
        assert_eq!(p.remote_of("main").unwrap().as_deref(), Some("origin"));
        assert!(p.rules_for("unit/x").unwrap().is_none());
    }

    /// 137, 56, S64 (design D22): a branch that takes pull requests on
    /// records each host keeps is refused alone, naming the branch, its
    /// remote and `[signers]`; every other branch reads, and the refused
    /// branch's remote still reads for routing. Once the policy names
    /// signers, it reads.
    #[test]
    fn a_pull_request_table_on_records_each_host_keeps_refuses_its_branch_alone() {
        let unkept = Policy::parse(
            "[branches.main]\nremote = \"origin\"\n[branches.main.pull_requests]\n\
             [branches.\"bolt/*\"]\nremote = \"origin\"\n",
        )
        .unwrap();
        let e = unkept.rules_for("main").unwrap_err();
        assert_eq!(
            e.downcast_ref::<NoIntake>(),
            Some(&NoIntake {
                branch: "main".into(),
                remote: "origin".into(),
            })
        );
        assert_eq!(
            format!("{e:#}"),
            "main takes pull requests, which only the host that keeps it on origin takes in, \
             and its records are each host's own: name [signers] so they are kept on origin \
             (137)"
        );
        assert!(unkept.rules_for("bolt/x").unwrap().is_some());
        assert_eq!(unkept.remote_of("main").unwrap().as_deref(), Some("origin"));
        assert_eq!(unkept.remote_of("unit/x").unwrap(), None);
        let kept = Policy::parse(
            "[signers.a]\nssh = \".config/sluice.signers\"\n\
             [branches.main]\nremote = \"origin\"\ntrust = [\"a\"]\n\
             [branches.main.pull_requests]\n",
        )
        .unwrap();
        let main = kept.rules_for("main").unwrap().unwrap();
        assert!(main.signed && !main.unkept && main.pull_requests.is_some());
    }

    /// 23: a landing commit's subject is the policy's, conventional by
    /// default.
    #[test]
    fn reads_the_landing_subject() {
        assert_eq!(Policy::parse("").unwrap().landing_subject, LANDING_SUBJECT);
        let p = Policy::parse("[landing]\nsubject = \"land: #{id} from {agent}\"\n").unwrap();
        assert_eq!(p.landing_subject, "land: #{id} from {agent}");
        assert!(Policy::parse("[landing]\nsubject = \"#{nope}\"\n").is_err());
        assert!(Policy::parse("[landing]\nsubject = \"#{id\"\n").is_err());
    }

    /// ADR 0036: the tables in the order the file writes them, which the
    /// overview lists branches by.
    #[test]
    fn keeps_the_branches_tables_in_the_order_written() {
        let p = Policy::parse(
            "[branches.zeta]\nrequired = []\n\
             [branches.\"bolt/*\"]\nrequired = []\n\
             [branches.alpha]\nrequired = []\n",
        )
        .unwrap();
        let order: Vec<&str> = p.branches.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(order, ["zeta", "bolt/*", "alpha"]);
    }

    /// 27 to 29: a gate names the results it trusts.
    #[test]
    fn reads_what_a_gate_trusts() {
        let p = Policy::parse(
            "[branches.main]\ntrust = [\"confined\", \"sandbox\"]\n\
             [branches.\"bolt/*\"]\ntrust = [\"open\", \"cache\"]\n",
        )
        .unwrap();
        let main = p.rules_for("main").unwrap().unwrap();
        assert!(main.trusts_run("sandbox", false));
        assert!(main.trusts_run("container", true));
        assert!(!main.trusts_run("open", false));
        assert!(!main.trusts("cache"));
        assert!(main.satisfiable_by("sandbox"));
        assert!(!main.satisfiable_by("open"));
        let bolt = p.rules_for("bolt/x").unwrap().unwrap();
        // 108: `cache` is read as absent, with a warning.
        assert!(!bolt.trusts("cache") && bolt.trusts_run("open", false));
        assert!(!bolt.satisfiable_by("sandbox"));
        assert_eq!(p.warnings.len(), 1);
        assert!(p.warnings[0].contains("\"cache\"") && p.warnings[0].contains("bolt/*"));
        // ADR 0015: a table without `trust` takes every run sluice starts,
        // and no replay.
        let default = Policy::parse("[branches.main]\n").unwrap();
        let default = default.rules_for("main").unwrap().unwrap();
        assert!(["open", "sandbox", "container"]
            .iter()
            .all(|k| default.satisfiable_by(k) && default.trusts_run(k, false)));
        assert!(!default.trusts("cache"));
        assert!(Policy::parse("[branches.main]\ntrust = [\"anyone\"]\n").is_err());
    }

    /// 52: an array of tables in a profile's table reaches the profile as
    /// it is written, given no meaning here.
    #[test]
    fn reads_an_array_of_tables_for_a_profile() {
        let p = Policy::parse("[[tests.service]]\nname = \"db\"\nports = [\"5432\"]\n").unwrap();
        assert_eq!(
            p.tests["service"],
            Setting::Tables(vec![Settings::from([
                ("name".into(), Setting::Text("db".into())),
                ("ports".into(), Setting::List(vec!["5432".into()])),
            ])])
        );
        assert!(Policy::parse("[[tests.service]]\nname = 1\n").is_err());
        assert!(Policy::parse("[[tests.service]]\n[[tests.service.inner]]\nx = \"y\"\n").is_err());
    }

    /// C.7: a branch may name the remote it lives on.
    #[test]
    fn reads_a_branchs_remote() {
        let p =
            Policy::parse("[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\n").unwrap();
        assert_eq!(
            p.rules_for("main").unwrap().unwrap().remote.as_deref(),
            Some("origin")
        );
        assert_eq!(p.rules_for("bolt/x").unwrap().unwrap().remote, None);
        assert!(Policy::parse("[branches.main]\nremote = 1\n").is_err());
        assert!(Policy::parse("[branches.main]\nremote = [\"origin\"]\n").is_err());
    }

    /// C.7: a branch may turn pull requests on, with the conditions a pull
    /// request meets before it is submitted; only on a remote.
    #[test]
    fn reads_a_branchs_pull_requests() {
        let p = Policy::parse(
            "[signers.a]\nssh = \".config/sluice.signers\"\n\
             [branches.main]\nremote = \"origin\"\ntrust = [\"a\"]\n\
             [branches.main.pull_requests]\nreviews = 1\nchecks = [\"qa/approved\"]\n\
             [branches.\"bolt/*\"]\n",
        )
        .unwrap();
        assert_eq!(
            p.rules_for("main").unwrap().unwrap().pull_requests,
            Some(PullRequests {
                reviews: 1,
                checks: vec!["qa/approved".into()],
                log_lines: None,
            })
        );
        // ADR 0044: a failed check's output posted only as asked.
        let asked = Policy::parse(
            "[signers.a]\nssh = \".config/sluice.signers\"\n\
             [branches.main]\nremote = \"origin\"\ntrust = [\"a\"]\n\
             [branches.main.pull_requests]\nlog_lines = 40\n",
        )
        .unwrap();
        assert_eq!(
            asked
                .rules_for("main")
                .unwrap()
                .unwrap()
                .pull_requests
                .as_ref()
                .unwrap()
                .log_lines,
            Some(40)
        );
        let err = Policy::parse(
            "[branches.main]\nremote = \"origin\"\n[branches.main.pull_requests]\nlog_lines = 0\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("log_lines"), "{err}");
        // Absent: work is submitted straight from a session's place.
        assert_eq!(p.rules_for("bolt/x").unwrap().unwrap().pull_requests, None);
        // An empty table takes every pull request out of draft.
        let p = Policy::parse(
            "[signers.a]\nssh = \".config/sluice.signers\"\n\
             [branches.main]\nremote = \"origin\"\ntrust = [\"a\"]\n\
             [branches.main.pull_requests]\n",
        )
        .unwrap();
        assert_eq!(
            p.rules_for("main").unwrap().unwrap().pull_requests,
            Some(PullRequests::default())
        );
        // Pull requests need the remote they are on.
        let err = Policy::parse("[branches.main]\n[branches.main.pull_requests]\nreviews = 1\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("main") && err.contains("remote"), "{err}");
        assert!(Policy::parse(
            "[branches.main]\nremote = \"origin\"\n[branches.main.pull_requests]\nreviews = -1\n"
        )
        .is_err());
        assert!(Policy::parse(
            "[branches.main]\nremote = \"origin\"\n[branches.main.pull_requests]\nlabels = [\"x\"]\n"
        )
        .is_err());
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(Policy::parse("[branches.main]\nwindow = 3\n").is_err());
        assert!(Policy::parse("[branches.main]\nparallel = 0\n").is_err());
        assert!(Policy::parse("[branches.main]\nflaky = \"retry\"\n").is_err());
        assert!(Policy::parse("[branches.main]\ntimeout = \"soon\"\n").is_err());
    }

    /// 123: how many more times a failed release is tried, 3 where the
    /// table names none; a negative number is refused, naming the table.
    #[test]
    fn reads_and_refuses_release_retries() {
        let rules = |text: &str| Policy::parse(text).map(|p| p.rules_for("main").unwrap().cloned());
        let retries = |text: &str| rules(text).unwrap().unwrap().release_retries;
        assert_eq!(retries("[branches.main]\n"), 3);
        assert_eq!(retries("[branches.main]\nrelease_retries = 1\n"), 1);
        assert_eq!(retries("[branches.main]\nrelease_retries = 0\n"), 0);
        let e = rules("[branches.main]\nrelease_retries = -1\n").unwrap_err();
        assert_eq!(
            e.to_string(),
            "[branches.\"main\"] release_retries must be at least 0, not -1"
        );
        assert!(Policy::parse("[branches.main]\nrelease_retries = \"3\"\n").is_err());
    }

    /// 83: the limits a run is held to, and the settings passed on.
    #[test]
    fn reads_the_executor_and_the_shell() {
        let p = Policy::parse(
            "[tests]\nrunner = \"tasks\"\nshell = [\"env-shell\", \"--\"]\n\
             [executor]\nkind = \"sandbox\"\nnetwork = false\nmemory = \"8G\"\n",
        )
        .unwrap();
        assert_eq!(
            p.tests["shell"],
            Setting::List(vec!["env-shell".into(), "--".into()])
        );
        assert_eq!(
            p.limits,
            Limits {
                executor: Some("sandbox".into()),
                memory: Some(8 << 30),
                network: Some(Network::Denied),
                docker: false,
            }
        );
        let network = |v: &str| {
            Policy::parse(&format!("[executor]\nnetwork = {v}\n")).map(|p| p.limits.network)
        };
        assert_eq!(network("true").unwrap(), Some(Network::Allowed));
        assert_eq!(network("\"localhost\"").unwrap(), Some(Network::Localhost));
        assert!(network("\"lan\"").is_err());
        assert!(network("1").is_err());
        assert!(
            Policy::parse("[executor]\ndocker = true\n")
                .unwrap()
                .limits
                .docker
        );
        assert!(Policy::parse("[executor]\ncpus = 2\n").is_err());
        assert!(Policy::parse("[executor]\nmemory = \"lots\"\n").is_err());
        assert!(Policy::parse("[executor]\nmemory = \"8\"\n").is_err());
        assert!(Policy::parse("[tests]\nshell = [1.5]\n").is_err());
        // A number in a list is its digits, as on its own: `not_run`'s
        // exits (could-not-run-1 114).
        assert_eq!(
            Policy::parse("[tests]\nnot_run = [77, 99]\n")
                .unwrap()
                .tests["not_run"],
            Setting::List(vec!["77".into(), "99".into()])
        );
        assert!(Policy::parse("[tests]\njobs = true\n").is_err());
        assert_eq!(
            Policy::parse("[checkout]\nports_from = 20000\n")
                .unwrap()
                .checkout["ports_from"],
            Setting::Text("20000".into())
        );
    }

    /// 35, 71; ADR 0049: a `[checkout]` that still declares a cache, in
    /// any shape, is refused, saying what replaced it; one that declares
    /// none reads as before.
    #[test]
    fn refuses_a_checkout_cache() {
        let refused = |text: &str| format!("{:#}", Policy::parse(text).unwrap_err());
        for text in [
            "[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n",
            "[checkout]\ncache = \"target\"\n",
        ] {
            let e = refused(text);
            assert!(
                e.contains("ADR 0049")
                    && e.contains("[[caches]]")
                    && e.contains("~/.config/sluice/config.toml")
                    && e.contains("Remove [[checkout.cache]] from .config/sluice.toml"),
                "{e}"
            );
        }
        let p = Policy::parse(
            "[checkout]\nmethod = \"git-worktree\"\nports = [\"web\"]\nports_from = 20000\n",
        )
        .unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(
            p.checkout,
            Settings::from([
                ("method".into(), Setting::Text("git-worktree".into())),
                ("ports".into(), Setting::List(vec!["web".into()])),
                ("ports_from".into(), Setting::Text("20000".into())),
            ])
        );
    }

    /// Previews-1 clauses 98, 99: `[preview]`'s command, addresses and
    /// time limit.
    #[test]
    fn reads_the_preview_table() {
        assert_eq!(Policy::parse("").unwrap().preview, None);
        let p = Policy::parse(
            "[preview]\ncommand = [\"moon\", \"run\", \"app:preview\"]\n\
             addresses = { web = \"https://{name.web}.localhost\", api = \"http://localhost:{port.api}/docs\" }\n\
             timeout = \"10m\"\n",
        )
        .unwrap()
        .preview
        .unwrap();
        assert_eq!(p.command, ["moon", "run", "app:preview"]);
        assert_eq!(p.addresses["web"], "https://{name.web}.localhost");
        assert_eq!(p.timeout, Some(Duration::from_secs(600)));
        let shell = Policy::parse("[preview]\ncommand = \"make preview\"\n").unwrap();
        assert_eq!(
            shell.preview.unwrap().command,
            ["/bin/sh", "-c", "make preview"]
        );
        let refused = |text: &str| format!("{:#}", Policy::parse(text).unwrap_err());
        assert!(refused("[preview]\n").contains("[preview] names no command"));
        assert!(refused("[preview]\ncommand = []\n").contains("a string or a list"));
        assert!(
            refused("[preview]\ncommand = \"x\"\naddresses = { web = \"{host}\" }\n")
                .contains("[preview] addresses web names {host}")
        );
        assert!(
            refused("[preview]\ncommand = \"x\"\naddresses = { web = \"{name.web\" }\n")
                .contains("leaves a { open")
        );
        assert!(refused("[preview]\ncommand = \"x\"\ntimeout = \"soon\"\n")
            .contains("[preview] timeout"));
        assert!(refused("[preview]\ncommand = \"x\"\nurls = {}\n").contains("urls"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("512M").unwrap(), 512 << 20);
        assert_eq!(parse_size("1G").unwrap(), 1 << 30);
        assert!(parse_size("1GB").is_err());
        assert!(parse_size("G").is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("20m").unwrap(), Duration::from_secs(1200));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("45s").unwrap(), Duration::from_secs(45));
        assert!(parse_duration("20").is_err());
    }

    #[test]
    fn an_exact_name_wins_over_a_pattern() {
        let p = Policy::parse(
            "[branches.\"bolt/*\"]\nparallel = 3\n[branches.\"bolt/x\"]\nparallel = 1\n",
        )
        .unwrap();
        assert_eq!(p.rules_for("bolt/x").unwrap().unwrap().parallel, 1);
        assert_eq!(p.rules_for("bolt/y").unwrap().unwrap().parallel, 3);
    }

    /// The statement's C.7 example: one repository's whole policy.
    const PEERS: &str = r##"
[tests]
runner = "moon"

[checkout]
ports = ["api", "web"]

[systems]
linux-x64   = { os = "linux", arch = "x86_64" }
linux-arm64 = { os = "linux", arch = "aarch64" }
macos-arm64 = { os = "macos", arch = "aarch64" }

[signers.box]
ssh = "exec@build-1 ssh-ed25519 AAAAC3Nza..."
confined = true
systems = ["linux-x64"]

[signers.forge]
github = { workflow = ".github/workflows/sluice.yml", ref = "refs/heads/main" }
confined = true
systems = ["linux-x64", "linux-arm64", "macos-arm64"]

[signers.sessions]
ssh = ".config/sluice.sessions"
confined = false

[pools.forge]
github = { workflow = ".github/workflows/sluice.yml" }
signer = "forge"
runners = { linux-x64 = "ubuntu-24.04", linux-arm64 = "ubuntu-24.04-arm", macos-arm64 = "macos-15" }

[branches."bolt/*"]
remote = "origin"
required = ["#check-fast"]
trust = ["sessions", "box", "forge"]

[branches.main]
remote = "origin"
required = ["#check-fast", "#integration", "cli:build"]
trust = ["box", "forge"]
confined = true
systems = ["linux-x64"]
batch = true
timeout = "45m"

[branches.main.systems_for]
"cli:build" = ["linux-x64", "linux-arm64", "macos-arm64"]

[branches.main.pull_requests]
reviews = 1
checks = ["codeql"]
"##;

    #[test]
    fn reads_the_peers_example_whole() {
        let p = Policy::parse(PEERS).unwrap();
        assert_eq!(p.systems.len(), 3);
        assert_eq!(
            p.systems["macos-arm64"],
            System {
                os: "macos".into(),
                arch: "aarch64".into()
            }
        );
        let forge = &p.signers["forge"];
        assert_eq!(forge.identity.0, "github");
        assert!(forge.confined && forge.may_claim("macos-arm64"));
        assert!(!p.signers["box"].may_claim("macos-arm64"));
        let sessions = &p.signers["sessions"];
        assert_eq!(
            sessions.identity,
            (
                "ssh".into(),
                Setting::Text(".config/sluice.sessions".into())
            )
        );
        assert!(!sessions.confined && sessions.may_claim("linux-arm64"));
        assert_eq!(p.pools.len(), 1);
        assert_eq!(p.pools[0].signer, "forge");
        assert_eq!(p.pools[0].runners["macos-arm64"], "macos-15");
        let main = p.rules_for("main").unwrap().unwrap();
        assert!(main.signed && main.confined);
        assert_eq!(main.trust, ["box", "forge"]);
        assert_eq!(main.systems_of("#check-fast"), ["linux-x64"]);
        assert_eq!(main.systems_of("cli:build").len(), 3);
        let bolt = p.rules_for("bolt/x").unwrap().unwrap();
        assert!(bolt.signed && !bolt.confined);
        assert!(bolt.systems_of("#check-fast").is_empty());
        assert!(main.satisfiable_by("open"));
    }

    fn refused(policy: &str) -> String {
        format!("{:#}", Policy::parse(policy).unwrap_err())
    }

    #[test]
    fn pools_keep_the_order_written() {
        let p = Policy::parse(&format!(
            "{PEERS}\n[pools.alpha]\ngithub = {{ workflow = \"a.yml\" }}\nsigner = \"forge\"\nrunners = {{ linux-x64 = \"x\" }}\n"
        ))
        .unwrap();
        let names: Vec<&str> = p.pools.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["forge", "alpha"]);
    }

    #[test]
    fn a_signer_has_exactly_one_identity() {
        let two = refused("[signers.box]\nssh = \"k\"\ngithub = { workflow = \"w\" }\n");
        assert!(
            two.contains("[signers.box]") && two.contains("more than one"),
            "{two}"
        );
        let none = refused("[signers.box]\nconfined = true\n");
        assert!(none.contains("[signers.box] names no identity"), "{none}");
    }

    #[test]
    fn an_unknown_system_is_refused_by_name() {
        let e = refused(&PEERS.replace(
            "systems = [\"linux-x64\"]\nbatch",
            "systems = [\"linux-riscv\"]\nbatch",
        ));
        assert!(
            e.contains("linux-riscv") && e.contains("[branches.\"main\"] systems"),
            "{e}"
        );
        let e = refused(&PEERS.replace(
            "\"cli:build\" = [\"linux-x64\",",
            "\"cli:build\" = [\"win\",",
        ));
        assert!(e.contains("\"win\""), "{e}");
        let e = refused(&PEERS.replace(
            "systems = [\"linux-x64\"]\n\n[signers.forge]",
            "systems = [\"x\"]\n\n[signers.forge]",
        ));
        assert!(e.contains("[signers.box]") && e.contains("\"x\""), "{e}");
    }

    #[test]
    fn a_pool_runner_its_signer_may_not_claim_is_refused() {
        let e = refused(&PEERS.replace(
            "systems = [\"linux-x64\", \"linux-arm64\", \"macos-arm64\"]\n\n[signers.sessions]",
            "systems = [\"linux-x64\", \"linux-arm64\"]\n\n[signers.sessions]",
        ));
        assert!(
            e.contains("[pools.forge]") && e.contains("macos-arm64"),
            "{e}"
        );
    }

    #[test]
    fn a_pool_signs_as_a_signer_of_its_kind() {
        let e = refused(&PEERS.replace("signer = \"forge\"", "signer = \"box\""));
        assert!(e.contains("[pools.forge]") && e.contains("own kind"), "{e}");
        let e = refused(&PEERS.replace("signer = \"forge\"", "signer = \"nobody\""));
        assert!(e.contains("nobody"), "{e}");
    }

    #[test]
    fn a_remote_branch_with_signers_trusts_signers_by_name() {
        let e = refused(&PEERS.replace("trust = [\"box\", \"forge\"]", "trust = [\"sandbox\"]"));
        assert!(
            e.contains("\"sandbox\"") && e.contains("trust lists signers"),
            "{e}"
        );
        let e = refused(&PEERS.replace("trust = [\"box\", \"forge\"]", "trust = [\"ci\"]"));
        assert!(e.contains("\"ci\", no signer"), "{e}");
        let p = Policy::parse(&PEERS.replace("trust = [\"box\", \"forge\"]\n", "")).unwrap();
        assert!(p.rules_for("main").unwrap().unwrap().trust.is_empty());
    }

    #[test]
    fn without_signers_a_remote_branch_keeps_its_kinds() {
        let p = Policy::parse(
            "[branches.main]\nremote = \"origin\"\ntrust = [\"confined\", \"sandbox\"]\nsystems = []\n",
        )
        .unwrap();
        let main = p.rules_for("main").unwrap().unwrap();
        assert!(!main.signed);
        assert_eq!(main.trust, ["confined", "sandbox"]);
        let e = refused("[branches.main]\nconfined = true\n");
        assert!(e.contains("list \"confined\" in trust"), "{e}");
    }

    #[test]
    fn the_lease_is_never_a_branchs() {
        for key in ["lease", "alive"] {
            let e = refused(&format!("[branches.main]\n{key} = \"1m\"\n"));
            assert!(
                e.contains(&format!("sets {key}")) && e.contains("[peers]"),
                "{e}"
            );
        }
    }

    /// 124: guarding passes over a table, a branch's field and a signer's
    /// field no sluice yet reads, each of which the full reading refuses.
    #[test]
    fn guarding_passes_over_what_it_does_not_need() {
        let table = "[later]\nx = 1\n";
        let field = "[branches.\"bolt/*\"]\nremote = \"origin\"\nlater = true\n";
        let signer = "[signers.box]\nssh = \"k\"\nlater = 1\n";
        for text in [table, field, signer] {
            assert!(Policy::parse(text).is_err(), "{text}");
        }
        let p = Policy::guarding(&format!("{table}{field}{signer}")).unwrap();
        let bolt = p.rules_for("bolt/x").unwrap().unwrap();
        assert_eq!(bolt.remote.as_deref(), Some("origin"));
        assert!(p.rules_for("main").unwrap().is_none());
        assert_eq!(p.signers.keys().collect::<Vec<_>>(), ["box"]);
        assert!(Policy::guarding("").unwrap().branches.is_empty());
    }

    /// 124: a value guarding needs, of the wrong kind, is an error naming
    /// it.
    #[test]
    fn guarding_refuses_what_it_needs_of_the_wrong_kind() {
        let e = format!(
            "{:#}",
            Policy::guarding("[branches.\"bolt/*\"]\nremote = 1\n").unwrap_err()
        );
        assert!(
            e.contains("[branches.\"bolt/*\"] remote") && e.contains('1'),
            "{e}"
        );
        let e = format!(
            "{:#}",
            Policy::guarding("[branches]\nmain = 1\n").unwrap_err()
        );
        assert!(e.contains("[branches.\"main\"] must be a table"), "{e}");
        let e = format!("{:#}", Policy::guarding("signers = 1\n").unwrap_err());
        assert!(e.contains("[signers] must be a table"), "{e}");
        assert!(Policy::guarding("[branches.\"bolt/*\"\nnot toml").is_err());
    }

    /// 124: guarding and the full reading agree on every example policy:
    /// the same patterns in the same order, each one's `remote`, and the
    /// same signers' names.
    #[test]
    fn guarding_reads_the_examples_as_parse_does() {
        let own = (
            "sluice's own",
            include_str!("../../../.config/sluice.toml"),
            "main",
        );
        let consts = [("POLICY", POLICY, "main"), ("PEERS", PEERS, "main")];
        for (name, text, _) in EXAMPLES.into_iter().chain([own]).chain(consts) {
            let full = Policy::parse(text).unwrap_or_else(|e| panic!("{DIR}/{name}: {e:#}"));
            let guarding = Policy::guarding(text).unwrap_or_else(|e| panic!("{DIR}/{name}: {e:#}"));
            let routes = |p: &Policy| -> Vec<(String, Option<String>)> {
                p.branches
                    .iter()
                    .map(|(n, r)| (n.clone(), r.remote.clone()))
                    .collect()
            };
            assert_eq!(routes(&guarding), routes(&full), "{name}");
            assert!(
                guarding.signers.keys().eq(full.signers.keys()),
                "{name}: {:?} and {:?}",
                guarding.signers.keys(),
                full.signers.keys()
            );
        }
    }
}
