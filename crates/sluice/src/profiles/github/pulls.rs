//! The pull request driver (C.5 pull request; `docs/adr/0021`): a branch
//! whose policy names `pull_requests` takes its work as pull requests on
//! GitHub. A pull request waits until it is out of draft and its
//! conditions hold on its head (C.7), and is then submitted through the
//! same `submit` as any session's work, from the place `pull/<n>`. Nothing
//! of a waiting pull request decides anything from the host's state: what
//! it keeps of each ([`Reading`]) only spares reading again what has not
//! moved (136).
//!
//! Intake runs before each round of such a branch, and asks GitHub for the
//! branch's open pull requests once the host's `[forge] discovery` has
//! passed since any process on the host asked, or at once after a submit
//! on the host moved one, the branch moved or its records did; otherwise
//! it asks nothing (136). When it asks, it submits each ready pull
//! request, withdraws the entry of one closed, back in draft or on a new
//! head, forecasts every waiting one, and posts each one's state as the
//! `sluice` check run on its head, only where what it says changed. A
//! pull request's reviews, checks and timeline are read again only once
//! its head, its last change on GitHub, its draft mark or its entry moved,
//! or, out of draft and still short of its conditions, at each asking.
//!
//! One host at a time takes a branch's pull requests in (137; rule
//! core.33; record 0093 as amended 2026-11-08): the instance that keeps
//! the branch on its remote, the lead its records name, any process of
//! that host taking them in under intake's lock, a pass, a step and a
//! submit's own first intake alike. A host that does not keep the branch
//! asks GitHub nothing about its open requests, withdraws nothing, and
//! follows its own requests by GitHub's word and sluice's check on their
//! heads (records 0095, 0096; design D21, D25).
//!
//! The driver's `session("pull/<n>")` fetches `refs/pull/<n>/head`; its
//! `notify` posts the outcome and, when a pull request is sent back,
//! comments once. Intake moves no branch (rule core.4): it fetches heads
//! and asks the core to submit, forecast and withdraw, as any driver does.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, SubsecRound, Utc};
use sluice_core::line::Line;
use sluice_core::policy::{Policy, PullRequests as Conditions, POLICY_PATH};
use sluice_core::records::{Event, State, Submission};
use sluice_core::{
    Clock, Forecast, GroupRequest, Requester, Seams, Sluice, SubmitHead, SubmitRefusal,
    SubmitRequest, SubmittedHead, Withdrew,
};
use sluice_seams::{
    Branch, CoveringVerdict, Decided, Delivery, Formed, HostCopy, LandingCommit, Moved,
    Notification, Rev, Session, Submitter,
};

use super::app::Repo;
use super::forge::{
    approvals, Check, Forge, Posted, Posting, Pull, Request, Review, Standing, Status, SLUICE_CHECK,
};
use super::kept::{Follow, Kept, Reading, Readings};
use super::render;
use crate::identity::Identity;
use crate::profiles::local_refs::LocalRefs;
use crate::profiles::pushed_refs::PushedRefs;
use crate::retry;
use crate::{Across, AcrossRow, Ahead, Followed, GroupRefused, Opened, Queue, Short, TakenInBy};
use sluice_core::verdict_view::{Standing as Stands, VerdictView};

/// The place a pull request's entry is submitted from.
pub fn place(number: u64) -> String {
    format!("pull/{number}")
}

/// The pull request a place names, if it names one.
pub fn number(worktree: &str) -> Option<u64> {
    worktree.strip_prefix("pull/")?.parse().ok()
}

/// The line `submit` writes in the body of a pull request it opens,
/// naming the agent whose work it is.
fn marker(agent: &str) -> String {
    format!("<!-- sluice: agent={agent} -->")
}

/// The agent a pull request's body names, if any.
fn marked_agent(body: &str) -> Option<String> {
    body.lines().find_map(|l| {
        l.trim()
            .strip_prefix("<!-- sluice: agent=")?
            .strip_suffix("-->")
            .map(|a| a.trim().to_string())
    })
}

/// `text` without any line that names an agent or a group.
fn unmarked(text: &str) -> String {
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            let l = l.trim();
            !l.starts_with("<!-- sluice: agent=") && !l.starts_with(GROUP_MARK)
        })
        .collect();
    kept.join("\n").trim_end().to_string()
}

/// `body` naming `agent` and, with `group`, marked one of that group, in
/// place of any agent and group it named.
fn with_marks(body: &str, agent: &str, group: Option<&Group>) -> String {
    let kept = unmarked(body);
    let mut marks = marker(agent);
    if let Some(g) = group {
        marks.push('\n');
        marks.push_str(&g.marker());
    }
    if kept.is_empty() {
        marks
    } else {
        format!("{kept}\n\n{marks}")
    }
}

/// The characters GitHub holds in a pull request's description: a longer
/// one it refuses with `422 Validation Failed`.
pub const DESCRIPTION_LIMIT: usize = 65_536;

/// GitHub's compare view of the request's branch `head_ref` against
/// `branch`, every commit of the request in full.
fn compare_address(repo: &Repo, branch: &str, head_ref: &str) -> String {
    format!(
        "https://github.com/{}/{}/compare/{branch}...{head_ref}",
        repo.owner, repo.name
    )
}

/// C.5 pull request: a request's description from its commits' messages,
/// oldest first, each whole, a blank line between them, then the marks,
/// when that fits [`DESCRIPTION_LIMIT`]. Otherwise the first messages, as
/// many as fit with every other commit named; then a line counting the
/// rest and linking `compare`, and each one's subject; then the marks.
/// When even every subject cannot be named, no message is whole, and the
/// subjects are named in order as many as fit, the others counted.
fn description(messages: &[String], agent: &str, group: Option<&Group>, compare: &str) -> String {
    let len = |s: &str| s.chars().count();
    let messages: Vec<String> = messages
        .iter()
        .map(|m| unmarked(m).trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    let all = with_marks(&messages.join("\n\n"), agent, group);
    if len(&all) <= DESCRIPTION_LIMIT {
        return all;
    }
    let n = messages.len();
    let subjects: Vec<&str> = messages
        .iter()
        .map(|m| m.lines().next().unwrap_or_default().trim())
        .collect();
    let count = |r: usize| {
        let s = if r == 1 { "" } else { "s" };
        format!("{r} more commit{s}, each by its subject; all of them in full: {compare}")
    };
    let others = |r: usize| format!("and {r} more, in the compare view");
    // Each length counts the line breaks before its part: the marks follow
    // a blank line, a named subject is `- <subject>` and its line break.
    let marks = 2 + len(&with_marks("", agent, group));
    let named = |s: &str| len(s) + 3;
    // The first `k` messages, each with the blank line after it, then the
    // count line, a blank line and the other subjects.
    let mut whole: usize = messages.iter().map(|m| len(m) + 2).sum();
    let mut rest = 0;
    for k in (0..n).rev() {
        whole -= len(&messages[k]) + 2;
        rest += named(subjects[k]);
        if whole + len(&count(n - k)) + 2 + rest - 1 + marks <= DESCRIPTION_LIMIT {
            let list: Vec<String> = subjects[k..].iter().map(|s| format!("- {s}")).collect();
            let mut parts = messages[..k].to_vec();
            parts.push(count(n - k));
            parts.push(list.join("\n"));
            return with_marks(&parts.join("\n\n"), agent, group);
        }
    }
    let mut room = len(&count(n)) + 2;
    let mut j = 0;
    while j < n && room + named(subjects[j]) + len(&others(n - j - 1)) + marks <= DESCRIPTION_LIMIT
    {
        room += named(subjects[j]);
        j += 1;
    }
    let mut list: Vec<String> = subjects[..j].iter().map(|s| format!("- {s}")).collect();
    list.push(others(n - j));
    with_marks(
        &format!("{}\n\n{}", count(n), list.join("\n")),
        agent,
        group,
    )
}

const GROUP_MARK: &str = "<!-- sluice: group=";

/// 125: a pull request opened by a group submit, as its body marks it:
/// the act's token, its place `k` of `n`, and the head it was opened at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    token: String,
    k: usize,
    n: usize,
    head: String,
}

impl Group {
    /// `<!-- sluice: group=<token> <k>/<n> head=<oid> -->`
    fn marker(&self) -> String {
        format!(
            "{GROUP_MARK}{} {}/{} head={} -->",
            self.token, self.k, self.n, self.head
        )
    }

    /// The group a pull request's body marks it one of, if any.
    fn of(body: &str) -> Option<Group> {
        body.lines().find_map(|l| {
            let rest = l.trim().strip_prefix(GROUP_MARK)?.strip_suffix("-->")?;
            let mut words = rest.split_whitespace();
            let token = words.next()?.to_string();
            let (k, n) = words.next()?.split_once('/')?;
            let head = words.next()?.strip_prefix("head=")?.to_string();
            let (k, n) = (k.parse().ok()?, n.parse().ok()?);
            (words.next().is_none() && (1..=n).contains(&k)).then_some(Group { token, k, n, head })
        })
    }
}

/// A group submit's token: when it was made and its first head, short.
fn group_token(at: DateTime<Utc>, first: &Rev) -> String {
    format!("{}-{}", at.format("%Y%m%dT%H%M%SZ"), first.short())
}

/// 125: a pull request bound to a group intake has not taken: the group's
/// size, what of the group it waits for, and what it lacks itself.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bound {
    n: usize,
    on: Vec<String>,
    missing: Vec<String>,
}

/// 125: what intake's pass over a branch's groups did: whether it took
/// any, what each bound request of a group not taken waits for, and what
/// failed for each it could not read (128).
struct Groups {
    taken: bool,
    bound: BTreeMap<u64, Bound>,
    unread: BTreeMap<u64, anyhow::Error>,
}

/// The line written into a comment, naming the notification it delivers,
/// so a repeat is known as one (51). Its key stays `told`, which the
/// comments on a forge already carry and a repeat is read by.
fn delivered_marker(id: &str) -> String {
    format!("<!-- sluice: told={id} -->")
}

/// What sluice knows of a pull request it read this round.
#[derive(Clone)]
struct Known {
    remote: String,
    pull: Pull,
}

/// A pull request's entry, for posting its outcome on its head.
#[derive(Clone)]
struct Entry {
    repo: Repo,
    number: u64,
    head: String,
}

pub struct Pulls {
    /// The repository's git directory.
    dir: PathBuf,
    /// The repository, as gix reads it, once opened: its default branch,
    /// its branches and its remotes' addresses, read with no process
    /// (129).
    local: std::cell::OnceCell<LocalRefs>,
    /// Where intake's lock files are kept.
    locks: PathBuf,
    forge: Option<Rc<dyn Forge>>,
    identity: Option<(Rc<dyn Identity>, PathBuf)>,
    /// The retry policy every call to a remote runs under (134).
    retry: Rc<retry::Policy>,
    /// The pull requests the last intake read, by place.
    known: RefCell<BTreeMap<String, Known>>,
    /// Every pull request entry of the branches intake read, by id.
    entries: RefCell<BTreeMap<u64, Entry>>,
    /// What each pull request's state reads, as last posted, by number.
    shown: RefCell<BTreeMap<u64, Shown>>,
    /// Each branch's `pull_requests.log_lines`, as intake last read it.
    log_lines: RefCell<BTreeMap<String, Option<usize>>>,
    /// What the host keeps of its calls to GitHub, each branch's readings
    /// among them (136).
    kept: Kept,
    /// How often a branch's open pull requests are asked for while
    /// nothing on the host moves (136).
    discovery: chrono::TimeDelta,
    /// What GitHub last said of each pull request a wait on the host that
    /// takes it in follows, by number, and when, with its timeline once
    /// read (136); one another host takes in is read through the host's
    /// kept follows instead (137, design D4).
    seen: RefCell<BTreeMap<u64, Seen>>,
    /// 129: the policies intake read, by revision, kept across its passes.
    policies: sluice_core::Policies,
    /// 129: the default branch's policy as read at a revision, and that
    /// revision: the policy, no policy file, or the words it was refused
    /// with.
    default_policy: RefCell<Option<(Rev, std::result::Result<Option<Policy>, String>)>>,
    /// This host as its peers name it, and its lease (72, 73, 137).
    peer: sluice_core::Peer,
    /// 128 (design D26): each cause the intake pass under way met, handed
    /// back by [`crate::PullRequests::intake`], printed by none of it, and
    /// whether it asked the forge for the branch's pull requests.
    causes: RefCell<Vec<crate::IntakeCause>>,
    asked: std::cell::Cell<bool>,
    /// Each branch another host keeps on its remote and takes the pull
    /// requests of in, with that host where its records name one, as this
    /// process last found it, by its intake or by following a request into
    /// it (137; design D25).
    away: RefCell<BTreeMap<String, Option<String>>>,
    /// 137, design D5: what this process's reader's documents last carried
    /// of the pull requests into each branch the host follows, by branch,
    /// read again only once the kept file or the mark moved.
    kept_across: RefCell<BTreeMap<String, KeptAcross>>,
}

/// 137, design D5: a branch's followed pull requests as a reader's
/// document carries them, and the kept file's modified time and the
/// mark's commit they were read at.
struct KeptAcross {
    modified: std::time::SystemTime,
    /// The host the branch's records named as keeping it (design D25).
    lead: Option<String>,
    across: Option<Across>,
}

/// A pull request as a wait last read it from GitHub, and its timeline
/// once read.
struct Seen {
    at: DateTime<Utc>,
    pull: Option<Pull>,
    readied: Option<Vec<DateTime<Utc>>>,
}

/// A pull request's state: what sluice posts, and what a submit that
/// follows it prints.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    posting: Posting,
    lines: Vec<String>,
}

/// What intake makes of one pull request this round: what to post on it,
/// if anything, and what of it sluice could not read, if anything.
struct Considered {
    shown: Option<Shown>,
    failed: Option<anyhow::Error>,
}

impl From<Shown> for Considered {
    fn from(shown: Shown) -> Considered {
        Considered {
            shown: Some(shown),
            failed: None,
        }
    }
}

/// A branch with pull requests, read at its revision.
struct Target {
    remote: String,
    repo: Repo,
    conditions: Conditions,
    tip: Rev,
    /// Whether its records are on its remote (C.6).
    signed: bool,
}

/// 137: whether this host takes a branch's pull requests in this pass.
enum Taking {
    /// It keeps the branch, and asks GitHub at once: a pass decides it only
    /// once it is due or the branch moved (129, 136).
    Here,
    /// Another host keeps it, by its name, or none does yet (design D21).
    Elsewhere(Option<String>),
}

impl Pulls {
    /// 128 (design D26): note a cause this intake pass met: what it asked
    /// and the words it was refused with.
    fn met(&self, asked: &str, said: String) {
        self.causes.borrow_mut().push(crate::IntakeCause {
            asked: asked.to_string(),
            said,
        });
    }

    /// The pull requests of the repository whose git directory is `dir`
    /// and whose common directory is `common`, read and posted on
    /// `forge`, and fetched as `identity` where it has one, each fetch and
    /// push under `retry`; each branch's open pull requests asked for
    /// every `discovery` while nothing on the host moves, and what was read
    /// of them kept in `kept` (136); `peer` is this host as its peers name
    /// it, with its lease (137).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dir: &Path,
        common: &Path,
        forge: Option<Rc<dyn Forge>>,
        identity: Option<(Rc<dyn Identity>, PathBuf)>,
        retry: Rc<retry::Policy>,
        kept: Kept,
        discovery: Duration,
        peer: sluice_core::Peer,
    ) -> Pulls {
        Pulls {
            dir: dir.to_path_buf(),
            local: std::cell::OnceCell::new(),
            locks: common.join("sluice").join(".intake"),
            forge,
            identity,
            retry,
            known: RefCell::default(),
            entries: RefCell::default(),
            shown: RefCell::default(),
            log_lines: RefCell::default(),
            kept,
            discovery: chrono::TimeDelta::from_std(discovery).unwrap_or(chrono::TimeDelta::MAX),
            seen: RefCell::default(),
            policies: sluice_core::Policies::default(),
            default_policy: RefCell::default(),
            peer,
            causes: RefCell::default(),
            asked: std::cell::Cell::new(false),
            away: RefCell::default(),
            kept_across: RefCell::default(),
        }
    }

    /// 129: start a pass: the policies intake read and the last pass did
    /// not use are forgotten.
    pub fn next_pass(&self) {
        self.policies.next_pass();
    }

    fn forge(&self, branch: &str) -> Result<&dyn Forge> {
        self.forge.as_deref().ok_or_else(|| {
            anyhow!(
                "{branch} takes pull requests, which sluice reads and posts as its GitHub App, \
                 and this host's config has no [forge] table naming it: see the guide \
                 \"Protecting main on GitHub\""
            )
        })
    }

    /// The repository, opened at its first reading.
    fn local(&self) -> Result<&LocalRefs> {
        if let Some(l) = self.local.get() {
            return Ok(l);
        }
        let l = LocalRefs::open(&self.dir)?;
        Ok(self.local.get_or_init(|| l))
    }

    fn pushed(&self, remote: &str) -> Result<PushedRefs> {
        PushedRefs::open_as(&self.dir, remote, self.identity.clone(), self.retry.clone())
    }

    /// `branch`'s pull requests and where they are, at its revision as
    /// the host holds it, with no fetch (129); `None` for a branch that
    /// takes none.
    fn target(&self, s: &Sluice<'_>, branch: &str) -> Result<Option<Target>> {
        let Some(tip) = s.seams().branch.held(branch)? else {
            return Ok(None);
        };
        self.target_at(s, branch, tip)
    }

    /// `branch`'s pull requests and where they are, at `tip`; `None` for a
    /// branch that takes none there.
    fn target_at(&self, s: &Sluice<'_>, branch: &str, tip: Rev) -> Result<Option<Target>> {
        let Some(rules) = s.rules_at(branch, &tip)? else {
            return Ok(None);
        };
        let signed = rules.signed;
        let (Some(conditions), Some(remote)) = (rules.pull_requests, rules.remote) else {
            return Ok(None);
        };
        let url = self.local()?.remote_url(&remote)?;
        let repo = Repo::from_url(&url).ok_or_else(|| {
            anyhow!(
                "{branch} takes pull requests, and its remote {remote} ({url}) is not on GitHub"
            )
        })?;
        Ok(Some(Target {
            remote,
            repo,
            conditions,
            tip,
            signed,
        }))
    }

    /// 137 (design D21): whether this host takes `t`'s pull requests in
    /// this pass: where the branch's records are on its remote and this
    /// instance keeps it, it does (72); otherwise the host its records name
    /// as their lead does, or none yet. A branch whose records each host
    /// keeps is taken in by none: its policy is refused (design D22).
    fn takes(&self, s: &Sluice<'_>, t: &Target, branch: &str) -> Result<Taking> {
        if t.signed && s.keeps(branch)? {
            return Ok(Taking::Here);
        }
        let lead = match t.signed {
            true => s.lead_of(branch)?,
            false => None,
        };
        Ok(Taking::Elsewhere(lead))
    }

    /// Each open pull request's latest submission among `open`, past one
    /// this host handed over to the host that took the branch's pull
    /// requests in, which stands for no act on the pull request (137).
    fn latest_of(
        &self,
        s: &Sluice<'_>,
        branch: &str,
        open: &[String],
    ) -> Result<BTreeMap<String, Submission>> {
        let mut latest = s.latest_of(branch, open)?;
        latest.retain(|_, e| !handed_over(e));
        Ok(latest)
    }

    /// The file of intake's lock on `branch`, and its path.
    fn lock_file(&self, branch: &str) -> Result<(std::fs::File, PathBuf)> {
        std::fs::create_dir_all(&self.locks)?;
        let path = self.locks.join(format!("{}.lock", lock_name(branch)));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| path.display().to_string())?;
        Ok((file, path))
    }

    /// Intake's lock on `branch`, or `None` when another process holds it:
    /// that process takes the branch's pull requests in this round.
    fn lock(&self, branch: &str) -> Result<Option<std::fs::File>> {
        let (file, path) = self.lock_file(branch)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e).context(path.display().to_string()),
        }
    }

    /// Intake's lock on `branch`, waited for: a submit holds it while it
    /// changes its pull request, so no pass reads it halfway.
    fn wait_lock(&self, branch: &str) -> Result<std::fs::File> {
        let (file, path) = self.lock_file(branch)?;
        file.lock().context(path.display().to_string())?;
        Ok(file)
    }

    /// The commit messages of `tip..head`, oldest first.
    fn messages(&self, tip: &Rev, head: &Rev) -> Result<Vec<String>> {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&self.dir)
            .args([
                "log",
                "--reverse",
                "--format=%B%x1e",
                &format!("{tip}..{head}"),
            ])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .output()
            .context("running git log")?;
        if !out.status.success() {
            bail!("git log: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .split('\u{1e}')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect())
    }

    /// Read every pull request entry of `subs` for later calls this round.
    fn register(&self, repo: &Repo, subs: &[Submission]) {
        let mut entries = self.entries.borrow_mut();
        for s in subs {
            if let Some(n) = number(&s.from.worktree) {
                entries.insert(
                    s.id,
                    Entry {
                        repo: repo.clone(),
                        number: n,
                        head: s.head.0.clone(),
                    },
                );
            }
        }
    }

    /// Post `shown` on `head`, unless sluice's check there already reads
    /// it, as `r` keeps what it last posted or read there (136).
    fn show(
        &self,
        forge: &dyn Forge,
        repo: &Repo,
        number: u64,
        head: &str,
        shown: Shown,
        r: &mut Reading,
    ) -> Result<()> {
        post_check(forge, repo, head, SLUICE_CHECK, &shown.posting, r)?;
        self.shown.borrow_mut().insert(number, shown);
        Ok(())
    }

    /// Comment `body` on pull request `number` unless a comment tells
    /// notification `id` already: one `r` keeps as said, or one its
    /// comments hold (136).
    fn comment_once(
        &self,
        forge: &dyn Forge,
        repo: &Repo,
        number: u64,
        id: &str,
        body: &str,
        r: &mut Reading,
    ) -> Result<()> {
        if r.said.iter().any(|s| s == id) {
            return Ok(());
        }
        let m = delivered_marker(id);
        let comments = forge.comment_list(repo, number)?;
        if comments.iter().any(|c| c.body.contains(&m)) {
            r.said.push(id.to_string());
            return Ok(());
        }
        // ADR 0044: a pull request sent back is pointed at its report.
        let report = comments
            .iter()
            .find(|c| c.body.starts_with("<!-- sluice-report: "))
            .map(|c| c.url.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| format!("https://github.com/{repo}/pull/{number}/checks"));
        forge.comment(
            repo,
            number,
            &format!("{m}\n{body}\n\nEvery check of the combination: [sluice's report]({report})."),
        )?;
        r.said.push(id.to_string());
        Ok(())
    }

    /// One intake pass over `branch`, reading the branch once: nothing
    /// asked of GitHub unless the host's interval has passed since any
    /// process on the host asked, a submit on the host moved one of its
    /// pull requests, or the branch as the host holds it or its records
    /// moved since (136). Whether any did is read with no fetch; the
    /// branch is fetched only once one did (129).
    fn take_in(&self, s: &Sluice<'_>, clock: &dyn Clock, branch: &str) -> Result<bool> {
        let Some(held) = self.target(s, branch)? else {
            return Ok(false);
        };
        let forge = self.forge(branch)?;
        // 137 (design D21): another process of the host takes this pass;
        // intake's one lock and the records' one lead make one taker.
        let Some(_lock) = self.lock(branch)? else {
            return Ok(true);
        };
        self.log_lines
            .borrow_mut()
            .insert(branch.to_string(), held.conditions.log_lines);
        let now = clock.now();
        let kept = self.kept.readings(&held.repo, branch);
        let records = s.seams().state.read_held(branch)?;
        let version = records.version;
        let due = kept.asked.is_none_or(|a| now - a >= self.discovery)
            || self.kept.moved_since(&held.repo, branch, kept.asked);
        // Whether the branch as held, and its records, stand as at the last
        // asking.
        let still =
            kept.tip.as_deref() == Some(held.tip.0.as_str()) && kept.version == Some(version);
        if !due && still {
            // A notification of this round finds its pull request.
            let line = Line::load(branch, records)?;
            let subs: Vec<Submission> = line.subs.into_values().collect();
            self.register(&held.repo, &subs);
            return Ok(true);
        }
        // 136: the branch where it is kept, fetched, and its pull requests
        // as its revision there names them.
        let Some(tip) = s.seams().branch.read(branch)? else {
            return Ok(false);
        };
        let t = match tip == held.tip {
            true => held,
            false => match self.target_at(s, branch, tip)? {
                Some(t) => t,
                None => return Ok(false),
            },
        };
        let pushed = self.pushed(&t.remote)?;
        // 137 (design D21): whether this host keeps the branch, before
        // anything is asked of GitHub; one that does not asks nothing and
        // withdraws nothing. One that does asks at once, the pass being due
        // or the branch moved.
        match self.takes(s, &t, branch)? {
            Taking::Here => {}
            Taking::Elsewhere(host) => {
                match t.signed {
                    true => self.away.borrow_mut().insert(branch.to_string(), host),
                    false => self.away.borrow_mut().remove(branch),
                };
                self.register(&t.repo, &s.status(branch)?.submissions);
                return Ok(true);
            }
        }
        self.away.borrow_mut().remove(branch);
        let at_tip = kept.tip.as_deref() == Some(t.tip.0.as_str());
        let still = at_tip && kept.version == Some(version);
        self.asked.set(true);
        let listed = forge.listed(&t.repo, branch)?;
        let (pulls, changed) = (listed.pulls, listed.changed);
        let mut status = s.status(branch)?;
        self.register(&t.repo, &status.submissions);
        let mut names = names(&status.submissions);

        // A submitted pull request closed, back in draft or on another
        // head leaves the queue. One that left the open list because the
        // branch as fetched holds its head, as another queue's landing
        // leaves it and GitHub reads as merged, has landed (122, 5): it is
        // never withdrawn, and the round finds it landed.
        let mut withdrew = false;
        for sub in &status.submissions {
            let Some(n) = number(&sub.from.worktree) else {
                continue;
            };
            if sub.state.is_out() || sub.state == State::Landing {
                continue;
            }
            let gone = match pulls.iter().find(|p| p.number == n) {
                None => !s.seams().branch.contains(&t.tip, &sub.head)?,
                Some(p) => p.draft || p.head != sub.head.0,
            };
            if gone {
                // Landing already: it lands, and the next round finds it so.
                let _ = s.withdraw_on(
                    &sub.branch,
                    sub.id,
                    &Requester::Session(sub.from.agent.clone()),
                )?;
                withdrew = true;
            }
        }
        if withdrew {
            status = s.status(branch)?;
        }
        let log = Line::read_log(&s.seams().state.read(branch)?)?;
        // Each open pull request's latest submission, finished or not: a
        // head that landed is never submitted again (131).
        let open: Vec<String> = pulls
            .iter()
            .filter(|p| p.from_repo(&t.repo))
            .map(|p| place(p.number))
            .collect();
        let mut latest = self.latest_of(s, branch, &open)?;
        let ours: Vec<&Pull> = pulls.iter().filter(|p| p.from_repo(&t.repo)).collect();
        for p in &ours {
            self.known.borrow_mut().insert(
                place(p.number),
                Known {
                    remote: t.remote.clone(),
                    pull: (*p).clone(),
                },
            );
        }
        // 136: what was read of each while it stands, else what is read
        // of it this pass.
        let work: RefCell<BTreeMap<u64, Reading>> = RefCell::new(
            ours.iter()
                .map(|p| {
                    let r = standing(&kept, p, latest.get(&place(p.number)), at_tip, still);
                    (p.number, r)
                })
                .collect(),
        );
        // 136: one query reads every open pull request whole, when their
        // list changed, while one out of draft waits on its conditions, or
        // when one has nothing kept; a draft is read by the list alone.
        let asks = ours.iter().any(|p| {
            !p.draft
                && (changed
                    || !kept.requests.contains_key(&p.number)
                    || waits(p, latest.get(&place(p.number)), &work.borrow()[&p.number]))
        });
        if asks {
            match forge.requests(&t.repo, branch) {
                Ok(read) => {
                    let mut work = work.borrow_mut();
                    for q in &read {
                        if let Some(r) = work.get_mut(&q.pull.number) {
                            read_whole(r, q, &t.conditions);
                        }
                    }
                }
                // GraphQL's allowance spent: the round passes the forge's
                // part by, and the next discovery asks again (134).
                Err(e) if retry::limited(&e).is_some() => {
                    self.keep_readings(s, &t, branch, now, &open, work.into_inner())?;
                    return Err(e);
                }
                // Refused otherwise: each is read alone this pass.
                Err(e) => self.met(
                    &format!("{branch}'s pull requests"),
                    format!(
                        "GitHub refused the query that reads them whole, so each is read \
                         alone this pass: {e:#}"
                    ),
                ),
            }
        }
        // 125: a group is taken whole, before any of it is considered alone.
        let groups = self.take_groups(s, forge, &t, &pushed, branch, &ours, &latest, &log, &work);
        let Groups {
            taken,
            bound,
            mut unread,
        } = match groups {
            Ok(g) => g,
            Err(e) => {
                self.keep_readings(s, &t, branch, now, &open, work.into_inner())?;
                return Err(e);
            }
        };
        if taken {
            status = s.status(branch)?;
            self.register(&t.repo, &status.submissions);
            names = self::names(&status.submissions);
            latest = self.latest_of(s, branch, &open)?;
        }
        for p in ours {
            // 128: what sluice reads of one pull request, or cannot,
            // decides that pull request alone; the next round reads it
            // again.
            let step = || -> Result<Option<anyhow::Error>> {
                let mut work = work.borrow_mut();
                let r = work
                    .get_mut(&p.number)
                    .expect("a reading of each open request");
                let c = self.consider(
                    s,
                    forge,
                    &t,
                    &pushed,
                    branch,
                    p,
                    latest.get(&place(p.number)),
                    bound.get(&p.number),
                    &status,
                    &log,
                    &names,
                    r,
                )?;
                if let Some(shown) = c.shown {
                    let report = match latest.get(&place(p.number)) {
                        Some(e) if e.head.0 == p.head => s.verdict_of(branch, e.id)?,
                        _ => None,
                    };
                    self.show_report(
                        forge,
                        &t.repo,
                        p.number,
                        &p.head,
                        t.conditions.log_lines,
                        &names,
                        shown,
                        report.as_ref(),
                        r,
                    )?;
                }
                Ok(c.failed)
            };
            // One of a group that could not be read is not considered
            // alone; the rest of its group waits for it.
            let stepped = match unread.remove(&p.number) {
                Some(e) => Ok(Some(e)),
                None => step(),
            };
            match stepped {
                Ok(None) => {}
                Ok(Some(e)) | Err(e) => self.met(&place(p.number), format!("{e:#}")),
            }
        }
        self.keep_readings(s, &t, branch, now, &open, work.into_inner())?;
        Ok(true)
    }

    /// 120, 136 (design D5): after a round of `branch`, each pull request
    /// a submit opened whose submission the round sent back, failed or in
    /// conflict, marked a draft and shown held for the next submit, as the
    /// next intake would show it, before any follower reads the pass. What
    /// the round sent back is told by the entry the host keeps of each
    /// request, which this pass's intake wrote, against its submission now;
    /// the request is the one intake last listed, at that submission's
    /// head. Nothing is asked of GitHub: no list, no query and no read of
    /// one request, its timeline but where the host keeps none, so a round
    /// that sent nothing back costs nothing (rule core.32). Its outcome
    /// and comment are the round's delivery's (50, 51), and a request
    /// opened by hand, which no submit holds, keeps them as they are
    /// posted. When the request was last asked for, at which revision and
    /// which version of the records, stands, so the next intake asks as it
    /// would have.
    fn settle_in(&self, s: &Sluice<'_>, branch: &str) -> Result<()> {
        if self.away.borrow().contains_key(branch) {
            return Ok(());
        }
        let Some(t) = self.target(s, branch)? else {
            return Ok(());
        };
        let Some(_lock) = self.lock(branch)? else {
            return Ok(());
        };
        let kept = self.kept.readings(&t.repo, branch);
        let queued: Vec<(u64, u64)> = kept
            .requests
            .iter()
            .filter_map(|(n, r)| {
                let (id, word) = r.entry.as_ref()?;
                let out = [
                    State::Landed,
                    State::Withdrawn,
                    State::Failed,
                    State::Conflict,
                ]
                .iter()
                .any(|x| x.word() == word);
                (!out).then_some((*n, *id))
            })
            .collect();
        if queued.is_empty() {
            return Ok(());
        }
        let places: Vec<String> = queued.iter().map(|(n, _)| place(*n)).collect();
        let latest = self.latest_of(s, branch, &places)?;
        let mut sent_back: Vec<(Pull, Submission)> = Vec::new();
        for (n, id) in &queued {
            let Some(e) = latest.get(&place(*n)) else {
                continue;
            };
            if e.id != *id || !matches!(e.state, State::Failed | State::Conflict) {
                continue;
            }
            let pull = self.known.borrow().get(&place(*n)).map(|k| k.pull.clone());
            if let Some(p) = pull.filter(|p| p.head == e.head.0 && submits(p).is_some()) {
                sent_back.push((p, e.clone()));
            }
        }
        if sent_back.is_empty() {
            return Ok(());
        }
        let forge = self.forge(branch)?;
        let status = s.status(branch)?;
        let log = Line::read_log(&s.seams().state.read(branch)?)?;
        let names = names(&status.submissions);
        let mut read: BTreeMap<u64, Reading> = BTreeMap::new();
        for (p, e) in &sent_back {
            let mut r = kept.requests.get(&p.number).cloned().unwrap_or_default();
            let held = is_held(p, Some(e), &log, || {
                timeline(forge, &t.repo, p.number, &mut r)
            });
            let held = match held {
                Ok(held) => held,
                Err(e) => {
                    self.met(&place(p.number), format!("{e:#}"));
                    continue;
                }
            };
            if !held {
                continue;
            }
            if !p.draft {
                match forge.set_draft(&t.repo, p.number, true) {
                    Ok(()) => r.draft = true,
                    Err(e) => self.met(
                        &place(p.number),
                        format!(
                            "it waits for the next submit, and GitHub would not mark it a draft: \
                             {e:#}"
                        ),
                    ),
                }
            }
            let queue = queue_at(&self.peer.name, Some(e), &status);
            let shown = waiting(branch, submits(p), &[NEXT_SUBMIT.to_string()], None, queue);
            let report = s.verdict_of(branch, e.id)?;
            let showed = self.show_report(
                forge,
                &t.repo,
                p.number,
                &p.head,
                t.conditions.log_lines,
                &names,
                shown,
                report.as_ref(),
                &mut r,
            );
            if let Err(e) = showed {
                self.met(&place(p.number), format!("{e:#}"));
            }
            r.entry = entry(Some(e));
            read.insert(p.number, r);
        }
        if read.is_empty() {
            return Ok(());
        }
        self.kept.change_readings(&t.repo, branch, |kept| {
            kept.requests.extend(read);
        })
    }

    /// 136: keep what this pass asked and read of `branch`'s pull
    /// requests: when it asked, at the branch's revision and its records'
    /// version as they stand once intake wrote, so its own submissions and
    /// withdrawals are no move; each open request's reading, with its
    /// entry as it stands now; and nothing of a request no longer open. A
    /// request this host's records hold at its head is followed no longer
    /// (137, design D3).
    fn keep_readings(
        &self,
        s: &Sluice<'_>,
        t: &Target,
        branch: &str,
        asked: DateTime<Utc>,
        open: &[String],
        mut work: BTreeMap<u64, Reading>,
    ) -> Result<()> {
        let latest = self.latest_of(s, branch, open)?;
        for (n, r) in &mut work {
            r.entry = entry(latest.get(&place(*n)));
        }
        let here: Vec<u64> = work
            .iter()
            .filter(|(n, r)| latest.get(&place(**n)).is_some_and(|e| e.head.0 == r.head))
            .map(|(n, _)| *n)
            .collect();
        self.unfollow(&t.repo, branch, &here);
        let version = s.seams().state.read(branch)?.version;
        self.kept.change_readings(&t.repo, branch, |kept| {
            kept.asked = Some(asked);
            kept.tip = Some(t.tip.0.clone());
            kept.version = Some(version);
            kept.requests = work;
        })
    }

    /// ADR 0044: post what pull request `p` shows of its submission: its
    /// `sluice` check, with the report as its text once a result touches
    /// it; the report as one comment in its conversation, edited in place;
    /// and a check per required check per system.
    #[allow(clippy::too_many_arguments)]
    fn show_report(
        &self,
        forge: &dyn Forge,
        repo: &Repo,
        number: u64,
        head: &str,
        log_lines: Option<usize>,
        names: &BTreeMap<u64, String>,
        mut shown: Shown,
        report: Option<&VerdictView>,
        r: &mut Reading,
    ) -> Result<()> {
        let Some(view) = report.filter(|v| render::has_report(v)) else {
            return self.show(forge, repo, number, head, shown, r);
        };
        let scrub = crate::scrub::Scrub::new(std::env::vars());
        let read_log = |at: &str| std::fs::read_to_string(at).ok();
        let cx = render::Context {
            names,
            repo_url: format!("https://github.com/{repo}"),
            log_lines,
            read_log: &read_log,
            scrub: &scrub,
        };
        let text = render::report(view, &cx);
        shown.posting.text = Some(text.clone());
        self.show(forge, repo, number, head, shown, r)?;

        // The comment: by the id the host keeps, else found by its marker,
        // created at the first result, edited after, never posted twice,
        // and edited only when what it says changed (136).
        let said = digest(&text);
        let mine = match r.report {
            Some(id) => Some((id, r.report_said.clone())),
            None => {
                let marker = render::marker(&view.branch);
                forge
                    .comment_list(repo, number)?
                    .into_iter()
                    .find(|c| c.body.starts_with(&marker))
                    .map(|c| (c.id, Some(digest(&c.body))))
            }
        };
        match mine {
            Some((id, before)) => {
                if before.as_deref() != Some(said.as_str()) {
                    if let Err(e) = forge.edit_comment(repo, id, &text) {
                        // Gone, perhaps: found again by its marker next time.
                        r.report = None;
                        return Err(e);
                    }
                }
                r.report = Some(id);
                r.report_said = Some(said);
            }
            None if view.standing != Stands::FormedAgain => {
                r.report = Some(forge.comment(repo, number, &text)?);
                r.report_said = Some(said);
            }
            None => {}
        }

        // 118, 140: a landing beside had no check run or replayed on its
        // head, so no check per required check says one was.
        if matches!(view.standing, Stands::Landed { beside: true, .. }) {
            return Ok(());
        }

        // A check per required check per system (many-hosts-3's C.5
        // addition), posted only where it differs from what was last
        // posted or read back.
        let mut named = Vec::new();
        for row in &view.rows {
            let (name, posting) = render::row_check(row, &cx);
            named.push(name.clone());
            post_check(forge, repo, head, &name, &posting, r)?;
        }
        for (task, system) in &view.pending {
            let name = super::forge::check_name(task, system.as_deref());
            if !named.contains(&name) {
                named.push(name.clone());
                let posting = render::pending_check(Status::InProgress, "testing");
                post_check(forge, repo, head, &name, &posting, r)?;
            }
        }
        // 20: one this pull request's combination left shows no outcome
        // of it; a check no longer among the rows waits again.
        let waits = if view.standing == Stands::FormedAgain {
            "formed again"
        } else {
            "testing"
        };
        let status = if view.standing == Stands::FormedAgain {
            Status::Queued
        } else {
            Status::InProgress
        };
        let left: Vec<String> = r
            .posted
            .keys()
            .filter(|name| *name != SLUICE_CHECK && !named.contains(name))
            .cloned()
            .collect();
        for name in left {
            post_check(
                forge,
                repo,
                head,
                &name,
                &render::pending_check(status, waits),
                r,
            )?;
        }
        Ok(())
    }

    /// Where pull request `p` stands this round, submitting it if it is
    /// ready; with the failure to fetch its head, if it could not be.
    #[allow(clippy::too_many_arguments)]
    fn consider(
        &self,
        s: &Sluice<'_>,
        forge: &dyn Forge,
        t: &Target,
        pushed: &PushedRefs,
        branch: &str,
        p: &Pull,
        latest: Option<&Submission>,
        group: Option<&Bound>,
        status: &sluice_core::Status,
        log: &[sluice_core::records::LogLine],
        names: &BTreeMap<u64, String>,
        r: &mut Reading,
    ) -> Result<Considered> {
        let here = place(p.number);
        // D1: its queue on this host, as sluice's check on its head
        // carries it, from its submission at that head, if any.
        let at = latest.filter(|e| e.head.0 == p.head);
        let queue = || queue_at(&self.peer.name, at, status);
        // A submit's pull request whose submission left the queue without
        // landing waits for its submitter's act, a submit or a person's
        // ready mark, at or after the second it left: whatever its head,
        // no round submits it in its agent's name before then (120, S56).
        let from = submits(p);
        let held = is_held(p, latest, log, || timeline(forge, &t.repo, p.number, r))?;
        // Its draft shows the hold, and is not the hold: one GitHub
        // refuses holds it all the same, and the next pass tries again.
        if held && !p.draft {
            if let Err(e) = forge.set_draft(&t.repo, p.number, true) {
                self.met(
                    &place(p.number),
                    format!(
                        "it waits for the next submit, and GitHub would not mark it a draft: {e:#}"
                    ),
                );
            }
        }
        // Who withdrew a hand-opened pull request, when it was someone
        // other than its own agent: it rests as one sent back does, and only
        // its person's act, a new head or a ready mark, offers it again
        // (120, C.5).
        let mut rests = None;
        if let Some(e) = at {
            match e.state {
                State::Withdrawn => {
                    if let Some(by) = withdrawn_by_another(e).filter(|_| from.is_none()) {
                        if !retried_by_hand(p, log, e.id, || timeline(forge, &t.repo, p.number, r))
                            .with_context(|| format!("reading {here}'s timeline"))?
                        {
                            rests = Some(by);
                        }
                    }
                }
                State::Landed => {
                    return Ok(outcome_shown(branch, &Outcome::of(e)?, names, queue()).into())
                }
                State::Failed | State::Conflict => {
                    // Sent back: one opened by hand is submitted again only
                    // once it is marked ready for review after it was sent
                    // back; a submit's, once it is no longer held.
                    let retried = match from {
                        Some(_) => !held,
                        None => {
                            retried_by_hand(p, log, e.id, || timeline(forge, &t.repo, p.number, r))
                                .with_context(|| format!("reading {here}'s timeline"))?
                        }
                    };
                    if !retried {
                        let outcome = Outcome::of(e)?;
                        if let Some(body) = comment(branch, &outcome, names, from) {
                            let id = format!("{}:{}", e.id, e.state.word());
                            self.comment_once(forge, &t.repo, p.number, &id, &body, r)?;
                        }
                        if !held {
                            return Ok(outcome_shown(branch, &outcome, names, queue()).into());
                        }
                    }
                }
                _ => return Ok(queue_shown(branch, e, status, names, queue()).into()),
            }
        }
        // Waiting: what is missing, and where it would land now. One of a
        // group waits for its group whole, and is taken only with it.
        let missing = if held {
            vec![NEXT_SUBMIT.to_string()]
        } else if let Some(by) = rests {
            vec![format!("{WITHDRAWN_BY}{}", withdrawer(by))]
        } else if let Some(g) = group {
            g.missing.clone()
        } else {
            self.conditions(forge, t, p, r)?
        };
        // 128: a waiting pull request needs its head only for its
        // forecast, and is shown waiting without one; one missing nothing
        // is not submitted, and nothing new is posted on it.
        if let Err(e) = self.fetch_head(pushed, p) {
            return Ok(Considered {
                shown: (p.draft || !missing.is_empty() || group.is_some()).then(|| match group {
                    Some(g) => waiting_in_group(branch, from, &missing, g, None, queue()),
                    None => waiting(branch, from, &missing, None, queue()),
                }),
                failed: Some(e),
            });
        }
        if missing.is_empty() && group.is_none() {
            // 23, 118, 120: its landing names the request's work by its
            // title.
            let accepted = s.submit(&SubmitRequest {
                head: SubmitHead::Worktree(place(p.number)),
                branch: branch.into(),
                after: Vec::new(),
                priority: 0,
                subject: Some(p.title.clone()),
            })?;
            return Ok(Considered::from(match accepted {
                Ok(a) => {
                    self.entries.borrow_mut().insert(
                        a.id,
                        Entry {
                            repo: t.repo.clone(),
                            number: p.number,
                            head: p.head.clone(),
                        },
                    );
                    // Ahead of it, each with its pull request: one this
                    // pass took in is in no status read before it, and is
                    // found among the entries it registered.
                    let pull_of = |i: u64| {
                        status
                            .submissions
                            .iter()
                            .find(|x| x.id == i)
                            .map(|x| number(&x.from.worktree))
                            .unwrap_or_else(|| self.entries.borrow().get(&i).map(|e| e.number))
                    };
                    let ahead: Vec<Ahead> = a
                        .ahead
                        .iter()
                        .map(|i| Ahead {
                            submission: *i,
                            pull_request: pull_of(*i),
                        })
                        .collect();
                    let named: Vec<String> = ahead
                        .iter()
                        .map(|x| match (names.get(&x.submission), x.pull_request) {
                            (None, Some(n)) => format!("#{n}"),
                            _ => name(x.submission, names),
                        })
                        .collect();
                    let queue = Queue {
                        host: self.peer.name.clone(),
                        taken_in: true,
                        submission: Some(a.id),
                        state: Some(State::Queued.word().to_string()),
                        ahead,
                        waits_for: Vec::new(),
                    };
                    queued(branch, &named, queue)
                }
                Err(refused) => waiting(
                    branch,
                    from,
                    &[format!("refused: {}", refusal_word(&refused))],
                    None,
                    queue(),
                ),
            }));
        }
        // 136: what was forecast stands while the branch, its records and
        // the request do.
        if let Some(forecast) = r.forecast.clone() {
            return Ok(match group {
                Some(g) => waiting_in_group(branch, from, &missing, g, Some(forecast), queue()),
                None => waiting(branch, from, &missing, Some(forecast), queue()),
            }
            .into());
        }
        let forecast = match s
            .forecast(&place(p.number), branch, &[])?
            .map(|f| f.forecast)
        {
            Ok(Forecast::Clear) => {
                let ahead: Vec<String> = status
                    .submissions
                    .iter()
                    .filter(|x| !x.state.is_out() && x.from.worktree != here)
                    .map(|x| name(x.id, names))
                    .collect();
                if ahead.is_empty() {
                    format!("forecast: merges onto {branch} without conflict")
                } else {
                    format!(
                        "forecast: merges without conflict behind {}",
                        ahead.join(", ")
                    )
                }
            }
            Ok(Forecast::Conflict { paths, with }) => {
                let with: Vec<String> = with.iter().map(|c| name_of(c.id, &c.from)).collect();
                let with = if with.is_empty() {
                    branch.to_string()
                } else {
                    with.join(", ")
                };
                format!("forecast: conflicts with {with} in {}", paths.join(", "))
            }
            Err(refused) => format!("forecast: none, {}", refusal_word(&refused)),
        };
        r.forecast = Some(forecast.clone());
        Ok(match group {
            Some(g) => waiting_in_group(branch, from, &missing, g, Some(forecast), queue()),
            None => waiting(branch, from, &missing, Some(forecast), queue()),
        }
        .into())
    }

    /// 136: what pull request `p` lacks before sluice takes it, as `r`
    /// keeps it while it stands and lacks nothing; read again otherwise,
    /// since another system's check moves nothing GitHub lists.
    fn conditions(
        &self,
        forge: &dyn Forge,
        t: &Target,
        p: &Pull,
        r: &mut Reading,
    ) -> Result<Vec<String>> {
        if let Some(met) = r.missing.as_ref().filter(|m| r.whole || m.is_empty()) {
            return Ok(met.clone());
        }
        let missing = self.missing(forge, t, p)?;
        r.missing = Some(missing.clone());
        Ok(missing)
    }

    /// What pull request `p` lacks before sluice takes it: out of draft,
    /// and its reviews and checks on its head (C.7).
    fn missing(&self, forge: &dyn Forge, t: &Target, p: &Pull) -> Result<Vec<String>> {
        let here = place(p.number);
        if p.draft {
            return Ok(lacks(&t.conditions, p, &[], &[]));
        }
        let reviews = forge
            .reviews(&t.repo, p.number)
            .with_context(|| format!("reading {here}'s reviews"))?;
        let checks = if t.conditions.checks.is_empty() {
            Vec::new()
        } else {
            forge
                .checks(&t.repo, &p.head)
                .with_context(|| format!("reading {here}'s checks"))?
        };
        Ok(lacks(&t.conditions, p, &reviews, &checks))
    }

    /// 125: each group of bound pull requests submitted whole, through one
    /// group submit in its order, once every one of it is open, out of
    /// draft, at its marked head and meeting its conditions; whether any
    /// was, what each request of a group not taken waits for, and what
    /// failed for each sluice could not read, which its group waits on
    /// (128). A request is bound to the group its body marks it one of
    /// while its head is the marked head and it has had no submission at
    /// that head.
    #[allow(clippy::too_many_arguments)]
    fn take_groups(
        &self,
        s: &Sluice<'_>,
        forge: &dyn Forge,
        t: &Target,
        pushed: &PushedRefs,
        branch: &str,
        pulls: &[&Pull],
        latest: &BTreeMap<String, Submission>,
        log: &[sluice_core::records::LogLine],
        work: &RefCell<BTreeMap<u64, Reading>>,
    ) -> Result<Groups> {
        let mut groups: BTreeMap<String, Vec<(Group, &Pull)>> = BTreeMap::new();
        for p in pulls {
            let Some(g) = Group::of(&p.body) else {
                continue;
            };
            let submitted = latest
                .get(&place(p.number))
                .is_some_and(|e| e.head.0 == p.head);
            if g.head == p.head && !submitted {
                groups.entry(g.token.clone()).or_default().push((g, *p));
            }
        }
        let mut taken = false;
        let mut bound = BTreeMap::new();
        let mut unread = BTreeMap::new();
        for members in groups.values() {
            let n = members[0].0.n;
            let mut by_k: BTreeMap<usize, &Pull> = BTreeMap::new();
            let mut agree = true;
            for (g, p) in members {
                agree &= g.n == n && by_k.insert(g.k, p).is_none();
            }
            // What each lacks: its submitter's act while it is held (120),
            // else its conditions.
            let mut missing = BTreeMap::new();
            for p in by_k.values() {
                let mut work = work.borrow_mut();
                let r = work
                    .get_mut(&p.number)
                    .expect("a reading of each open request");
                let held = is_held(p, latest.get(&place(p.number)), log, || {
                    timeline(forge, &t.repo, p.number, r)
                });
                let lacks = match held {
                    Ok(true) => Ok(vec![NEXT_SUBMIT.to_string()]),
                    Ok(false) => self.conditions(forge, t, p, r),
                    Err(e) => Err(e),
                };
                match lacks {
                    Ok(m) => {
                        missing.insert(p.number, m);
                    }
                    Err(e) => {
                        unread.insert(p.number, e);
                    }
                }
            }
            let absent: Vec<usize> = (1..=n).filter(|k| !by_k.contains_key(k)).collect();
            let ready = missing.len() == by_k.len() && missing.values().all(Vec::is_empty);
            let mut refused = None;
            if agree
                && absent.is_empty()
                && ready
                && by_k.values().all(|p| match self.fetch_head(pushed, p) {
                    Ok(_) => true,
                    Err(e) => {
                        unread.insert(p.number, e);
                        false
                    }
                })
            {
                let req = GroupRequest {
                    branch: branch.into(),
                    heads: by_k
                        .values()
                        .map(|p| SubmitHead::Worktree(place(p.number)))
                        .collect(),
                    after: Vec::new(),
                    priority: 0,
                    subjects: by_k.values().map(|p| p.title.clone()).collect(),
                };
                match s.submit_group(&req)? {
                    Ok(accepted) => {
                        for (a, p) in accepted.iter().zip(by_k.values()) {
                            self.entries.borrow_mut().insert(
                                a.id,
                                Entry {
                                    repo: t.repo.clone(),
                                    number: p.number,
                                    head: p.head.clone(),
                                },
                            );
                        }
                        taken = true;
                        continue;
                    }
                    Err(r) => refused = r.first().map(|(_, r)| refusal_word(r)),
                }
            }
            // Each of the group that waits, on what it lacks or on being
            // read again.
            let waits: BTreeMap<u64, bool> = by_k
                .values()
                .map(|p| {
                    let lacks = missing.get(&p.number).is_some_and(|m| !m.is_empty());
                    (p.number, lacks || unread.contains_key(&p.number))
                })
                .collect();
            for p in by_k.values() {
                let Some(lacks) = missing
                    .get(&p.number)
                    .filter(|_| !unread.contains_key(&p.number))
                else {
                    continue;
                };
                let mut on: Vec<String> = waits
                    .iter()
                    .filter(|(number, w)| **number != p.number && **w)
                    .map(|(number, _)| format!("#{number}"))
                    .collect();
                on.extend(
                    absent
                        .iter()
                        .map(|k| format!("the request marked {k}/{n}, not open")),
                );
                if !agree {
                    on.push("its group's marks, which disagree".into());
                }
                if let Some(r) = &refused {
                    on.push(format!("the group, refused: {r}"));
                }
                bound.insert(
                    p.number,
                    Bound {
                        n,
                        on,
                        missing: lacks.clone(),
                    },
                );
            }
        }
        Ok(Groups {
            taken,
            bound,
            unread,
        })
    }

    /// 125: the act's pull requests, `acted`, each at the head the act
    /// pushed and with when its last submission left the queue. When every
    /// one meets its conditions there, the reviews and checks the branch
    /// names, each is marked ready as one submit marks it and all are
    /// queued in one write in the act's order, while the act holds intake's
    /// lock, so no pass takes one of them alone (rules core.6, core.14).
    /// Out of draft is the act's to give, so each is read as out of draft.
    /// Otherwise none is marked ready, each ready one is marked a draft,
    /// and none is queued: the refusal names each short of its conditions
    /// (120).
    #[allow(clippy::too_many_arguments)]
    fn take_act(
        &self,
        s: &Sluice<'_>,
        forge: &dyn Forge,
        t: &Target,
        branch: &str,
        worktrees: &[String],
        acted: &[(Pull, Option<DateTime<Utc>>)],
        mut opened: Vec<Opened>,
    ) -> Result<Result<Vec<Opened>, GroupRefused>> {
        let mut short = Vec::new();
        for (w, (p, _)) in worktrees.iter().zip(acted) {
            let ready = Pull {
                draft: false,
                ..p.clone()
            };
            let lacks = self.missing(forge, t, &ready)?;
            if !lacks.is_empty() {
                short.push(Short {
                    worktree: w.clone(),
                    number: p.number,
                    lacks,
                });
            }
        }
        if !short.is_empty() {
            self.leave_drafts(forge, t, acted, &mut opened)?;
            return Ok(Err(GroupRefused::Short { opened, short }));
        }
        // Every head is here before any is marked ready, so one GitHub
        // serves none of leaves every request as it was.
        let pushed = self.pushed(&t.remote)?;
        for (p, _) in acted {
            self.fetch_head(&pushed, p)
                .with_context(|| format!("fetching #{}'s head", p.number))?;
        }
        for ((p, left), o) in acted.iter().zip(&mut opened) {
            if p.draft || left.is_some() {
                mark_ready(forge, &t.repo, p.number, p.draft, *left)?;
                o.said.push_str(" · marked ready for review");
            }
            // The driver's session of `pull/<n>` answers from what is known.
            self.known.borrow_mut().insert(
                place(p.number),
                Known {
                    remote: t.remote.clone(),
                    pull: Pull {
                        draft: false,
                        ..p.clone()
                    },
                },
            );
        }
        let req = GroupRequest {
            branch: branch.into(),
            heads: acted
                .iter()
                .map(|(p, _)| SubmitHead::Worktree(place(p.number)))
                .collect(),
            after: Vec::new(),
            priority: 0,
            subjects: acted.iter().map(|(p, _)| p.title.clone()).collect(),
        };
        let accepted = match s.submit_group(&req)? {
            Ok(accepted) => accepted,
            Err(refused) => {
                // Refused by the branch since it was read: left drafts, as
                // any refused act leaves them, each refusal its worktree's.
                let drafted: Vec<(Pull, Option<DateTime<Utc>>)> = acted
                    .iter()
                    .map(|(p, left)| {
                        (
                            Pull {
                                draft: false,
                                ..p.clone()
                            },
                            *left,
                        )
                    })
                    .collect();
                self.leave_drafts(forge, t, &drafted, &mut opened)?;
                let refused = refused
                    .into_iter()
                    .map(|(at, r)| {
                        let w = req
                            .heads
                            .iter()
                            .position(|x| x.name() == at)
                            .map_or(at, |i| worktrees[i].clone());
                        (w, r)
                    })
                    .collect();
                return Ok(Err(GroupRefused::Worktrees(refused)));
            }
        };
        for ((a, (p, _)), o) in accepted.into_iter().zip(acted).zip(&mut opened) {
            self.entries.borrow_mut().insert(
                a.id,
                Entry {
                    repo: t.repo.clone(),
                    number: p.number,
                    head: p.head.clone(),
                },
            );
            o.submission = Some(a);
        }
        Ok(Ok(opened))
    }

    /// 125: each of a refused act's pull requests left a draft at its
    /// head, so no round takes one of it alone, by its conditions or by its
    /// group's marks, in its submitter's name (120).
    fn leave_drafts(
        &self,
        forge: &dyn Forge,
        t: &Target,
        acted: &[(Pull, Option<DateTime<Utc>>)],
        opened: &mut [Opened],
    ) -> Result<()> {
        for ((p, _), o) in acted.iter().zip(opened) {
            if !p.draft {
                forge
                    .set_draft(&t.repo, p.number, true)
                    .with_context(|| format!("marking #{} a draft", p.number))?;
                o.said.push_str(" · marked a draft");
            }
        }
        Ok(())
    }

    /// The pull request's head on this host, fetched unless it is here.
    fn fetch_head(&self, pushed: &PushedRefs, p: &Pull) -> Result<Rev> {
        let local = self.head_ref(pushed.remote(), p.number);
        if let Some(r) = pushed.read_ref(&local)? {
            if r.0 == p.head {
                return Ok(r);
            }
        }
        let got = pushed
            .fetch_ref(&format!("refs/pull/{}/head", p.number), &local)?
            .ok_or_else(|| anyhow!("{} has no head on {}", place(p.number), pushed.remote()))?;
        if got.0 != p.head {
            bail!(
                "{} moved to {} while sluice read it at {}; the next round takes its new head",
                place(p.number),
                got.short(),
                &p.head[..p.head.len().min(7)]
            );
        }
        Ok(got)
    }

    /// Where a pull request's head is kept on this host.
    fn head_ref(&self, remote: &str, number: u64) -> String {
        format!("refs/sluice/{remote}/pull/{number}")
    }
}

/// 136: sluice's check run `name` on `head` posted as `posting`, unless
/// it reads so already: as `r` keeps what was last posted or read on that
/// head, and as GitHub holds it where `r` keeps nothing of that head.
fn post_check(
    forge: &dyn Forge,
    repo: &Repo,
    head: &str,
    name: &str,
    posting: &Posting,
    r: &mut Reading,
) -> Result<()> {
    if r.posted_on.as_deref() != Some(head) {
        r.posted = forge
            .posted_all(repo, head)?
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();
        r.posted_on = Some(head.to_string());
    }
    let existing = r.posted.get(name).map(|p| p.id);
    if r.posted.get(name).map(|p| &p.posting) != Some(posting) {
        let id = forge.post_named(repo, head, name, existing, posting)?;
        r.posted.insert(
            name.to_string(),
            Posted {
                id,
                name: name.to_string(),
                posting: posting.clone(),
            },
        );
    }
    Ok(())
}

/// What pull request `p` lacks of `conditions` before sluice takes it, by
/// `reviews` and the `checks` on its head: out of draft, and its reviews
/// and checks (C.7).
fn lacks(conditions: &Conditions, p: &Pull, reviews: &[Review], checks: &[Check]) -> Vec<String> {
    let mut missing = Vec::new();
    if p.draft {
        missing.push("a draft".to_string());
        return missing;
    }
    let have = approvals(reviews, &p.head);
    if have < conditions.reviews {
        let need = conditions.reviews - have;
        missing.push(format!("{need} review{}", if need == 1 { "" } else { "s" }));
    }
    for name in &conditions.checks {
        if !checks
            .iter()
            .any(|c| &c.name == name && c.standing == Standing::Passed)
        {
            missing.push(name.clone());
        }
    }
    missing
}

/// 136: whether pull request `p` waits on its conditions, as `r` read
/// them: it has no submission at its head, and lacked something when last
/// read, or was not read. Another system's check moves nothing GitHub
/// lists, so such a request is read again at each discovery.
fn waits(p: &Pull, latest: Option<&Submission>, r: &Reading) -> bool {
    latest.is_none_or(|e| e.head.0 != p.head) && r.missing.as_ref().is_none_or(|m| !m.is_empty())
}

/// 136: fill `r` from `q`, the pull request as the one query read it
/// whole, when it is at the head the list gave: what it lacks of
/// `conditions`, its ready marks, and sluice's check runs on its head, so
/// nothing of it is read alone this pass.
fn read_whole(r: &mut Reading, q: &Request, conditions: &Conditions) {
    if r.head != q.pull.head || r.draft != q.pull.draft {
        return;
    }
    r.missing = Some(lacks(conditions, &q.pull, &q.reviews, &q.checks));
    r.readied = Some(q.readied.clone());
    r.posted = q
        .posted
        .iter()
        .map(|p| (p.name.clone(), p.clone()))
        .collect();
    r.posted_on = Some(q.pull.head.clone());
    r.whole = true;
}

/// What a report comment says, kept short: the first 32 hex digits of its
/// SHA-256.
fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A pull request's latest submission as a reading keeps it: its id and
/// its state's word.
fn entry(latest: Option<&Submission>) -> Option<(u64, String)> {
    latest.map(|e| (e.id, e.state.word().to_string()))
}

/// 136: what is known of `p` this pass. While its head, last change,
/// draft mark and latest submission are as last read, the reading kept
/// stands: what it lacked while the branch stands at the revision it was
/// read at (`at_tip`), whose policy names the conditions, and its forecast
/// while the branch's records stand too (`still`). Otherwise only what
/// sluice posted on it and commented is carried on, and everything else is
/// read again.
fn standing(
    kept: &Readings,
    p: &Pull,
    latest: Option<&Submission>,
    at_tip: bool,
    still: bool,
) -> Reading {
    let entry = entry(latest);
    let fresh = Reading {
        head: p.head.clone(),
        updated_at: p.updated_at,
        draft: p.draft,
        entry: entry.clone(),
        ..Reading::default()
    };
    match kept.requests.get(&p.number) {
        Some(r)
            if r.head == p.head
                && r.updated_at == p.updated_at
                && r.draft == p.draft
                && r.entry == entry =>
        {
            let mut r = r.clone();
            if !at_tip {
                r.missing = None;
            }
            if !still {
                r.forecast = None;
            }
            r
        }
        Some(r) => Reading {
            posted_on: r.posted_on.clone(),
            posted: r.posted.clone(),
            report: r.report,
            report_said: r.report_said.clone(),
            said: r.said.clone(),
            ..fresh
        },
        None => fresh,
    }
}

/// 137, design D3: an open pull request at `head` as this host's own act
/// on it knows it, `draft` or not, titled `title` and opened from `from`,
/// with nothing yet read of it.
fn known(head: &str, draft: bool, title: &str, from: &str) -> Follow {
    Follow {
        read_at: None,
        head: head.to_string(),
        open: true,
        merged: false,
        draft,
        title: title.to_string(),
        from: from.to_string(),
        check: None,
    }
}

/// A combination's name in a pull request's words: `main + #38 + #41`.
fn combination(branch: &str, ids: &[u64], names: &BTreeMap<u64, String>) -> String {
    let mut s = branch.to_string();
    for i in ids {
        s.push_str(&format!(" + {}", name(*i, names)));
    }
    s
}

/// What each submission is called on a pull request: `#<n>` for a pull
/// request's, its number otherwise.
fn names(subs: &[Submission]) -> BTreeMap<u64, String> {
    subs.iter()
        .map(|s| (s.id, name_of(s.id, &s.from)))
        .collect()
}

fn name_of(id: u64, from: &Submitter) -> String {
    match number(&from.worktree) {
        Some(n) => format!("#{n}"),
        None => format!("submission {id} ({})", from.agent),
    }
}

fn name(id: u64, names: &BTreeMap<u64, String>) -> String {
    names
        .get(&id)
        .cloned()
        .unwrap_or_else(|| format!("submission {id}"))
}

fn refusal_word(r: &SubmitRefusal) -> String {
    match r {
        SubmitRefusal::Dirty(_) => "its head holds more than its commits".into(),
        SubmitRefusal::Detached => "its head is on no branch".into(),
        SubmitRefusal::NoBranch => "its branch is gone".into(),
        SubmitRefusal::NotManaged => "no [branches] table names its branch".into(),
        SubmitRefusal::UnknownAfter(_) => "it names an unknown submission".into(),
        SubmitRefusal::NoHead(_) => "its head's branch is gone".into(),
        SubmitRefusal::ToItself => "its head is the branch it lands on".into(),
        SubmitRefusal::NotLandedOn => "its head is no branch sluice lands on".into(),
        SubmitRefusal::NamedTwice(_) => "it is named twice".into(),
        SubmitRefusal::KeptOut(kept) => format!(
            "its branch requires {}, which the task runner keeps out of CI",
            kept.iter()
                .map(|k| k.task.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// When submission `id` left the queue without landing, withdrawn, blamed
/// or found conflicting, from the log: the last such line, past a
/// resubmission's withdrawal of one sent back already. That withdrawal is
/// written by the act that submitted its pull request again, after the
/// submit's ready mark; read as its leaving, it would hold the request
/// that mark brought back, and intake would mark it a draft and withdraw
/// the new submission (120, S56).
fn removal(log: &[sluice_core::records::LogLine], id: u64) -> Option<DateTime<Utc>> {
    let leaving = |l: &&sluice_core::records::LogLine| match &l.event {
        Event::Withdrawn { id: i, .. }
        | Event::Blamed { id: i, .. }
        | Event::Conflict { id: i, .. } => *i == id,
        _ => false,
    };
    let resubmitted = |l: &&sluice_core::records::LogLine| matches!(&l.event, Event::Withdrawn { by, .. } if by == RESUBMISSION);
    log.iter()
        .rev()
        .filter(leaving)
        .find(|l| !resubmitted(l))
        .or_else(|| log.iter().rev().find(leaving))
        .and_then(|l| DateTime::parse_from_rfc3339(&l.at).ok())
        .map(|t| t.with_timezone(&Utc))
}

/// 120, 137: whether `e` was withdrawn by its host's hand-over of its
/// branch's pull requests to another host, which holds no request: its
/// withdrawal's record names the host it handed over to (record 0088 as
/// amended, design D1).
fn handed_over(e: &Submission) -> bool {
    let Some(w) = e.withdrawn.as_ref().filter(|_| e.state == State::Withdrawn) else {
        return false;
    };
    w.handed_over_to.is_some() || named_no_host(w, &e.branch)
}

/// A hand-over's withdrawal as a sluice that named no host in its record
/// wrote it, 0.60.0 to 0.70.2: in the words it gave every hand-over and
/// no other withdrawal, `<host> takes <branch>'s pull requests in`, or
/// `another host takes ...` before 0.70.1, which are all such a record
/// says of it. It is read as that sluice read it, holding no request,
/// as 120 has every hand-over's withdrawal (design D2).
fn named_no_host(w: &sluice_core::records::Withdrawal, branch: &str) -> bool {
    w.handed_over_to.is_none()
        && w.by
            .ends_with(&format!(" takes {branch}'s pull requests in"))
}

/// Who a resubmission's withdrawal names: the core withdraws the earlier
/// submissions from a place as it queues its new one (47, 67).
const RESUBMISSION: &str = "resubmission";

/// What a pull request a submit opened or updated waits for once its
/// submission left the queue.
const NEXT_SUBMIT: &str = "the next submit";

/// The title of sluice's check on a pull request held for [`NEXT_SUBMIT`],
/// which a host that does not take the requests in reads it by (record
/// 0096).
fn held_title() -> String {
    format!("waiting: {NEXT_SUBMIT}")
}

/// How the title of sluice's check on a pull request blamed begins, and on
/// one in conflict, which a host that does not take the requests in reads
/// it by (record 0096).
const BLAMED: &str = "failed: ";
const CONFLICTED: &str = "conflict: ";

/// The branch a submit's pull request is submitted from, `None` for one a
/// person opened by hand: a submit names its agent in the body, and is
/// the act that stands for the pull request from then on.
fn submits(p: &Pull) -> Option<&str> {
    marked_agent(&p.body).map(|_| p.head_ref.as_str())
}

/// What a pull request opened by hand waits for once someone other than
/// its own agent withdrew it, before who that was.
const WITHDRAWN_BY: &str = "withdrawn by ";

/// Who withdrew `e`, when it was not its own agent: the operator, a
/// planner or another agent, never intake, which withdraws a pull
/// request's submission as its agent, and never a hand-over, which holds
/// no request (120).
fn withdrawn_by_another(e: &Submission) -> Option<&str> {
    e.withdrawn
        .as_ref()
        .filter(|_| !handed_over(e))
        .map(|w| w.by.as_str())
        .filter(|by| *by != e.from.agent)
}

/// A withdrawal's `by` in a pull request's words.
fn withdrawer(by: &str) -> String {
    match by {
        "operator" | "planner" => format!("the {by}"),
        agent => agent.to_string(),
    }
}

/// Out of the queue without landing: withdrawn, blamed or conflicting;
/// never withdrawn for a hand-over of intake, which is none of these and
/// holds no request: whichever host holds the mark takes it in again as
/// its conditions hold, the host that withdrew it included (120, 137).
fn left_unlanded(e: &Submission) -> bool {
    matches!(e.state, State::Withdrawn | State::Failed | State::Conflict) && !handed_over(e)
}

/// Whether a submit's pull request `p` waits for its submitter's act: its
/// latest submission, `latest`, left the queue without landing, and no
/// ready mark has followed since (120, S56).
/// `readied` reads when it was marked ready for review; a draft is held
/// with no read, since only a ready mark, which ends its draft, lets it go
/// (136).
fn is_held(
    p: &Pull,
    latest: Option<&Submission>,
    log: &[sluice_core::records::LogLine],
    readied: impl FnOnce() -> Result<Vec<DateTime<Utc>>>,
) -> Result<bool> {
    match latest.filter(|e| submits(p).is_some() && left_unlanded(e)) {
        Some(_) if p.draft => Ok(true),
        Some(e) => {
            let readied =
                readied().with_context(|| format!("reading {}'s timeline", place(p.number)))?;
            Ok(!readied_since(&readied, acts_from(log, e)))
        }
        None => Ok(false),
    }
}

/// 136: when pull request `number` was marked ready for review, as `r`
/// keeps its timeline while it stands, else read.
fn timeline(
    forge: &dyn Forge,
    repo: &Repo,
    number: u64,
    r: &mut Reading,
) -> Result<Vec<DateTime<Utc>>> {
    if let Some(readied) = &r.readied {
        return Ok(readied.clone());
    }
    let readied = forge.readied(repo, number)?;
    r.readied = Some(readied.clone());
    Ok(readied)
}

/// The second from which a ready mark is its submitter's act after `e`
/// left the queue: the second it left, from the log, as GitHub and the
/// log both keep whole seconds. Never the second `e` was submitted in,
/// whose ready mark is the act that submitted it.
fn acts_from(log: &[sluice_core::records::LogLine], e: &Submission) -> DateTime<Utc> {
    let submitted = DateTime::parse_from_rfc3339(&e.submitted)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::MIN_UTC);
    let left = removal(log, e.id).unwrap_or(submitted);
    if left > submitted {
        left
    } else {
        submitted + chrono::TimeDelta::seconds(1)
    }
}

/// Whether a pull request marked ready for review at `readied` was so at
/// or after `at`, to the whole second.
fn readied_since(readied: &[DateTime<Utc>], at: DateTime<Utc>) -> bool {
    readied.iter().any(|r| r.trunc_subsecs(0) >= at)
}

/// Whether pull request `p`, opened by hand and sent back as submission
/// `id`, or withdrawn by someone other than its agent, was marked ready
/// for review after it left, as `readied` reads its timeline: intake then
/// submits the same head again.
fn retried_by_hand(
    p: &Pull,
    log: &[sluice_core::records::LogLine],
    id: u64,
    readied: impl FnOnce() -> Result<Vec<DateTime<Utc>>>,
) -> Result<bool> {
    let removed = removal(log, id);
    Ok(!p.draft && readied()?.iter().any(|at| removed.is_some_and(|r| *at > r)))
}

/// A submit's ready mark on pull request `number`, a draft first unless
/// it is one, so that GitHub's timeline holds a ready mark at or after
/// `left`, the second from which it is the act after its submission left
/// the queue. GitHub's clock may be behind the host's: a mark read back
/// before `left` is made again the next second, three times at most.
fn mark_ready(
    forge: &dyn Forge,
    repo: &Repo,
    number: u64,
    draft: bool,
    left: Option<DateTime<Utc>>,
) -> Result<()> {
    const TRIES: usize = 3;
    let mut draft = draft;
    let mut newest = None;
    for tried in 1..=TRIES {
        if !draft {
            forge.set_draft(repo, number, true)?;
        }
        forge.set_draft(repo, number, false)?;
        draft = false;
        let Some(left) = left else {
            return Ok(());
        };
        newest = forge
            .readied(repo, number)?
            .into_iter()
            .map(|r| r.trunc_subsecs(0))
            .max();
        if newest.is_some_and(|r| r >= left) {
            return Ok(());
        }
        if tried < TRIES {
            let into = Utc::now().timestamp_subsec_millis();
            std::thread::sleep(std::time::Duration::from_millis(u64::from(
                1000 - into.min(999),
            )));
        }
    }
    let at = |t: DateTime<Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let left = left.map(at).unwrap_or_default();
    match newest {
        Some(r) => bail!(
            "#{number}'s ready mark reads {} on GitHub, and a submit's counts from {left}, when \
             its submission left the queue, after {TRIES} tries: GitHub's clock may be behind \
             this host's; submit again",
            at(r)
        ),
        None => bail!(
            "GitHub's timeline of #{number} holds no ready mark after {TRIES} tries, and a \
             submit's counts from {left}, when its submission left the queue; submit again"
        ),
    }
}

/// The way on for a submit's pull request held for its submitter: the next
/// submit from `from`, or a ready mark.
fn next_submit(branch: &str, from: &str) -> String {
    format!(
        "The next `sluice submit {branch}` from `{from}`, or marking it ready for review, \
         submits it again."
    )
}

/// D1: where the block that carries a request's queue opens in sluice's
/// check's summary, an HTML comment GitHub's Markdown hides.
const QUEUE_BLOCK: &str = "<!-- sluice:queue ";

/// 6, 7: the submissions ahead of `e` in its branch's queue as `status`
/// reads it, in the order a round takes them (D1).
fn ahead_in<'a>(e: &Submission, status: &'a sluice_core::Status) -> Vec<&'a Submission> {
    status
        .ahead_of(e.id)
        .into_iter()
        .filter_map(|i| status.submissions.iter().find(|x| x.id == i))
        .collect()
}

/// D1: a pull request's queue on `host`, as sluice's check on its head
/// carries it: `at` is the submission of the request at that head that
/// `host`'s records hold, if any, and `status` the branch's queue there,
/// which gives those ahead of it, in queue order, while it is in the
/// queue. It waits for nothing until a waiting check names what.
fn queue_at(host: &str, at: Option<&Submission>, status: &sluice_core::Status) -> Queue {
    let ahead = match at {
        Some(e) if !e.state.is_out() => ahead_in(e, status)
            .into_iter()
            .map(|x| Ahead {
                submission: x.id,
                pull_request: number(&x.from.worktree),
            })
            .collect(),
        _ => Vec::new(),
    };
    Queue {
        host: host.to_string(),
        taken_in: at.is_some(),
        submission: at.map(|e| e.id),
        state: at.map(|e| e.state.word().to_string()),
        ahead,
        waits_for: Vec::new(),
    }
}

/// D1: the queue a check's summary carries in its last block; `None`
/// where it carries none, as an older sluice posted it, or where that
/// block does not read.
pub fn queue_of(summary: &str) -> Option<Queue> {
    let rest = &summary[summary.rfind(QUEUE_BLOCK)? + QUEUE_BLOCK.len()..];
    serde_json::from_str(&rest[..rest.find(" -->")?]).ok()
}

/// What sluice's check on a pull request's head says of it, for a host
/// that follows it across hosts (137, C.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Told {
    /// Not decided, or not taken in.
    Open,
    Landed,
    /// Failed, or a conflict.
    SentBack {
        conflict: bool,
    },
    /// Held for [`NEXT_SUBMIT`], its submission having left the queue
    /// (120; record 0096).
    Held,
}

/// Design D6: the fields decide over the words. A check that carries its
/// block is read from it alone: waiting for the next submit is held,
/// taken in and `failed` or `conflict` is sent back, `landed` is landed,
/// and anything else is open, whatever GitHub kept of a run it completed
/// earlier. A check with no block, as an older sluice posted it, is read
/// by its title, never its status, as record 0096 reads it: `failed: …`
/// sent back, `conflict: …` sent back as a conflict, and `waiting: the
/// next submit` held.
fn told(posting: Option<&Posting>) -> Told {
    let Some(posting) = posting else {
        return Told::Open;
    };
    match queue_of(&posting.summary) {
        Some(q) if q.waits_for.iter().any(|w| w == NEXT_SUBMIT) => Told::Held,
        Some(q) if q.taken_in => match q.state.as_deref() {
            Some("landed") => Told::Landed,
            Some("failed") => Told::SentBack { conflict: false },
            Some("conflict") => Told::SentBack { conflict: true },
            _ => Told::Open,
        },
        Some(_) => Told::Open,
        None if posting.title == held_title() => Told::Held,
        None if posting.title.starts_with(BLAMED) => Told::SentBack { conflict: false },
        None if posting.title.starts_with(CONFLICTED) => Told::SentBack { conflict: true },
        None => Told::Open,
    }
}

/// D1: a check's summary as a person reads it, its block left out.
fn words(summary: &str) -> String {
    summary
        .lines()
        .filter(|l| !l.trim_start().starts_with(QUEUE_BLOCK))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// D1: a check's summary: a first line naming the request's submission
/// on the posting host, or that it waits to be taken in there; `words`;
/// and the block that carries `queue`, its `>` escaped so that no field
/// ends the comment.
fn summed(queue: &Queue, words: &str) -> String {
    let first = first_line(queue);
    let fields = serde_json::to_string(queue)
        .unwrap_or_default()
        .replace('>', "\\u003e");
    format!("{first}\n\n{words}\n\n{QUEUE_BLOCK}{fields} -->")
}

/// D1: a check's first line: the request's submission on the posting
/// host, or that it waits to be taken in there.
fn first_line(queue: &Queue) -> String {
    match queue.submission.filter(|_| queue.taken_in) {
        Some(n) => format!("Submission {n} on {}.", queue.host),
        None => format!("Waiting to be taken in on {}.", queue.host),
    }
}

/// What a waiting pull request into `branch` is titled, the words of its
/// summary before any forecast, and what it waits for in its title's
/// words: `from` is the branch a submit's pull request is submitted from.
fn waiting_words(
    branch: &str,
    from: Option<&str>,
    missing: &[String],
) -> (String, Vec<String>, Vec<String>) {
    let held = missing.iter().any(|m| m == NEXT_SUBMIT);
    let rests = missing.iter().find_map(|m| m.strip_prefix(WITHDRAWN_BY));
    let draft = missing.iter().any(|m| m == "a draft");
    let (title, waits_for) = if held {
        (held_title(), vec![NEXT_SUBMIT.to_string()])
    } else if let Some(who) = rests {
        let w = format!("{WITHDRAWN_BY}{who}");
        (format!("waiting: {w}"), vec![w])
    } else if draft {
        ("waiting: a draft".to_string(), vec!["a draft".to_string()])
    } else {
        (
            format!("waiting: needs {}", missing.join(", ")),
            missing.to_vec(),
        )
    };
    let words = if held {
        format!(
            "Its submission left the queue, and sluice marked it a draft. {}",
            next_submit(branch, from.unwrap_or("its branch"))
        )
    } else if let Some(who) = rests {
        format!(
            "Withdrawn from {branch}'s queue by {who}. New commits, or marking it a draft and \
             then ready for review, submit it again."
        )
    } else if draft {
        "A draft: sluice takes it once it is marked ready for review.".to_string()
    } else {
        let named: Vec<String> = missing
            .iter()
            .map(|m| {
                if m.ends_with("review") || m.ends_with("reviews") {
                    m.clone()
                } else {
                    format!("`{m}`")
                }
            })
            .collect();
        format!(
            "Needs {} on this commit before sluice takes it.",
            named.join(", ")
        )
    };
    (title, vec![words], waits_for)
}

/// What a waiting pull request into `branch` shows: `from` is the branch
/// a submit's pull request is submitted from, and `queue` its queue on
/// the host that posts it (D1).
fn waiting(
    branch: &str,
    from: Option<&str>,
    missing: &[String],
    forecast: Option<String>,
    mut queue: Queue,
) -> Shown {
    let (title, mut summary, waits_for) = waiting_words(branch, from, missing);
    let mut lines = Vec::new();
    if let Some(f) = forecast {
        summary.push(f.replacen("forecast:", "Forecast:", 1));
        lines.push(f);
    }
    queue.waits_for = waits_for;
    Shown {
        posting: Posting {
            text: None,
            status: Status::Queued,
            title,
            summary: summed(&queue, &summary.join("\n\n")),
        },
        lines,
    }
}

/// 125: one of a group intake has not taken, which waits for what it
/// lacks itself, or else for the rest of its group: `from` is the branch
/// it is submitted from, and `queue` its queue on the host that posts it
/// (D1).
fn waiting_in_group(
    branch: &str,
    from: Option<&str>,
    missing: &[String],
    g: &Bound,
    forecast: Option<String>,
    mut queue: Queue,
) -> Shown {
    let together = format!(
        "One of a group of {} submitted together, which sluice takes only all together, once \
         each is ready.",
        g.n
    );
    let (title, mut summary, waits_for) = if missing.is_empty() {
        let on = g.on.join(", ");
        (
            format!("waiting for its group: {on}"),
            vec![format!("{together} It waits for {on}.")],
            vec![format!("its group: {on}")],
        )
    } else {
        let (title, mut summary, waits_for) = waiting_words(branch, from, missing);
        summary.push(together);
        (title, summary, waits_for)
    };
    let mut lines = Vec::new();
    if let Some(f) = forecast {
        summary.push(f.replacen("forecast:", "Forecast:", 1));
        lines.push(f);
    }
    queue.waits_for = waits_for;
    Shown {
        posting: Posting {
            text: None,
            status: Status::Queued,
            title,
            summary: summed(&queue, &summary.join("\n\n")),
        },
        lines,
    }
}

/// A pull request queued on `branch` behind `ahead`, as each is named on
/// a pull request: `queue` is its queue on the host that posts it (D1).
fn queued(branch: &str, ahead: &[String], queue: Queue) -> Shown {
    let title = if ahead.is_empty() {
        format!("queued: next on {branch}")
    } else {
        format!("queued behind {}", ahead.join(", "))
    };
    Shown {
        posting: Posting {
            text: None,
            status: Status::Queued,
            summary: summed(
                &queue,
                &format!("In {branch}'s queue; it lands once its combination passes."),
            ),
            title,
        },
        lines: Vec::new(),
    }
}

/// A submitted pull request's state in the queue, `status` the branch's
/// queue, whose order names what it is queued behind: `queue` is its
/// queue on the host that posts it (D1).
fn queue_shown(
    branch: &str,
    e: &Submission,
    status: &sluice_core::Status,
    names: &BTreeMap<u64, String>,
    queue: Queue,
) -> Shown {
    match (&e.tested, e.state) {
        (Some(t), State::Testing | State::Passed | State::Landing) => {
            let title = format!("testing {}", combination(branch, &t.submissions, names));
            Shown {
                posting: Posting {
                    text: None,
                    status: Status::InProgress,
                    summary: summed(
                        &queue,
                        &format!(
                            "sluice is testing `{}`, {branch} with what is ahead of it.",
                            t.combination.short()
                        ),
                    ),
                    title,
                },
                lines: Vec::new(),
            }
        }
        _ => {
            let ahead: Vec<String> = ahead_in(e, status)
                .iter()
                .map(|x| name(x.id, names))
                .collect();
            queued(branch, &ahead, queue)
        }
    }
}

/// What became of a submission, from its record or its notification.
enum Outcome {
    Landed {
        landed_as: Rev,
        /// 140: beside the queue, with no check run.
        beside: bool,
        /// 140: beside the queue, the result that covers the revision it
        /// landed on; `None` otherwise, and in outcomes recorded before it
        /// (record 0083).
        covered_by: Option<CoveringVerdict>,
    },
    Failed {
        tasks: Vec<String>,
        logs: Vec<(String, String)>,
        combination: Rev,
        ids: Vec<u64>,
        timed_out: bool,
    },
    Conflict {
        paths: Vec<String>,
        with: Vec<String>,
    },
}

impl Outcome {
    fn of(sub: &Submission) -> Result<Outcome> {
        Ok(match sub.state {
            State::Landed => Outcome::Landed {
                landed_as: sub.landed_as.clone().unwrap_or_else(|| sub.head.clone()),
                beside: sub.beside == Some(sluice_core::records::Beside::Landed),
                covered_by: sub.covered_by.clone(),
            },
            State::Failed => {
                let t = sub
                    .tested
                    .as_ref()
                    .ok_or_else(|| anyhow!("submission {} was blamed untested", sub.id))?;
                Outcome::Failed {
                    tasks: sub.failed.iter().map(|f| f.task.clone()).collect(),
                    logs: sub
                        .failed
                        .iter()
                        .filter_map(|f| Some((f.task.clone(), f.log.clone()?)))
                        .collect(),
                    combination: t.combination.clone(),
                    ids: t.submissions.clone(),
                    timed_out: sub.result.as_deref() == Some("timed out"),
                }
            }
            State::Conflict => {
                let c = sub
                    .conflict
                    .as_ref()
                    .ok_or_else(|| anyhow!("submission {} conflicts with nothing", sub.id))?;
                Outcome::Conflict {
                    paths: c.paths.clone(),
                    with: c.with.iter().map(|w| name_of(w.id, &w.from)).collect(),
                }
            }
            other => bail!("submission {} is {}", sub.id, other.word()),
        })
    }

    fn decided(decided: &Decided) -> Outcome {
        match decided.clone() {
            Decided::Landed {
                landed_as,
                beside,
                covered_by,
            } => Outcome::Landed {
                landed_as,
                beside,
                covered_by,
            },
            Decided::Failed {
                failed,
                combination,
                submissions,
                timed_out,
                logs,
            } => Outcome::Failed {
                tasks: failed,
                logs: logs.into_iter().map(|l| (l.task, l.text)).collect(),
                combination,
                ids: submissions,
                timed_out,
            },
            Decided::Conflict { paths, with } => Outcome::Conflict {
                paths,
                with: with.iter().map(|w| name_of(w.id, &w.from)).collect(),
            },
        }
    }

    /// The state the submission's record names for it.
    fn state(&self) -> State {
        match self {
            Outcome::Landed { .. } => State::Landed,
            Outcome::Failed { .. } => State::Failed,
            Outcome::Conflict { .. } => State::Conflict,
        }
    }
}

/// What became of a submission, on its pull request: `queue` is its
/// queue on the host that posts it (D1).
fn outcome_shown(branch: &str, o: &Outcome, names: &BTreeMap<u64, String>, queue: Queue) -> Shown {
    let (status, title, summary) = match o {
        Outcome::Landed {
            landed_as,
            beside: false,
            ..
        } => (
            Status::Success,
            format!("landed as {}", landed_as.short()),
            format!("Landed on {branch} as `{}`.", landed_as.short()),
        ),
        // 140: no required check reads its changes; the result that
        // covers it named as its landing commit names it (23; record
        // 0083), where the outcome carries it.
        Outcome::Landed {
            landed_as,
            beside: true,
            covered_by,
        } => (
            Status::Success,
            format!("landed as {} beside the queue", landed_as.short()),
            format!(
                "Landed on {branch} as `{}` beside the queue, with no check run: no required \
                 check reads its changes.{}",
                landed_as.short(),
                covered_by
                    .as_ref()
                    .map(|c| format!(
                        " Covered by the result at `{}`, {}.",
                        c.at.short(),
                        sluice_core::text::verdict_words(&c.result, c.ran, c.replayed)
                    ))
                    .unwrap_or_default()
            ),
        ),
        Outcome::Failed {
            tasks,
            combination: c,
            ids,
            timed_out,
            ..
        } => (
            Status::Failure,
            if *timed_out {
                format!("{BLAMED}timed out")
            } else {
                format!("{BLAMED}{}", tasks.join(", "))
            },
            format!(
                "`{}` failed on {} (`{}`), and this pull request is to blame.",
                tasks.join("`, `"),
                combination(branch, ids, names),
                c.short()
            ),
        ),
        Outcome::Conflict { paths, with } => (
            Status::Failure,
            format!("{CONFLICTED}{}", paths.join(", ")),
            format!(
                "`{}` conflicts with {}.",
                paths.join("`, `"),
                if with.is_empty() {
                    branch.to_string()
                } else {
                    with.join(", ")
                }
            ),
        ),
    };
    Shown {
        posting: Posting {
            text: None,
            status,
            title,
            summary: summed(&queue, &summary),
        },
        lines: Vec::new(),
    }
}

/// The comment on a pull request sent back; none for a landing. `from` is
/// the branch a submit's pull request is submitted from: sluice marks it a
/// draft, and only its submitter's act brings it back.
fn comment(
    branch: &str,
    o: &Outcome,
    names: &BTreeMap<u64, String>,
    from: Option<&str>,
) -> Option<String> {
    let drafted = from.map(|f| format!("sluice marked it a draft. {}", next_submit(branch, f)));
    let again = match &drafted {
        Some(d) => d.clone(),
        None => "Push a fix to submit again, or mark this pull request a draft and then ready \
                 for review to submit the same commits again."
            .to_string(),
    };
    match o {
        Outcome::Landed { .. } => None,
        Outcome::Failed {
            tasks,
            logs,
            combination: c,
            ids,
            timed_out,
        } => {
            let mut s = if *timed_out {
                format!(
                    "sluice sent this pull request back: {} (`{}`) timed out, and it is to blame.",
                    combination(branch, ids, names),
                    c.short()
                )
            } else {
                format!(
                    "sluice sent this pull request back: `{}` failed on {} (`{}`), and it is to blame.",
                    tasks.join("`, `"),
                    combination(branch, ids, names),
                    c.short()
                )
            };
            if !logs.is_empty() {
                s.push_str("\n\nThe logs, on the host that runs sluice:\n");
                for (task, at) in logs {
                    s.push_str(&format!("\n- `{task}`: `{at}`"));
                }
            }
            s.push_str(&format!("\n\n{again}"));
            Some(s)
        }
        Outcome::Conflict { paths, with } => {
            let way = match &drafted {
                Some(d) => format!("Merge or rebase onto {branch}. {d}"),
                None => format!(
                    "Merge or rebase onto {branch} and push, or mark this pull request a draft \
                     and then ready for review to submit the same commits again."
                ),
            };
            Some(format!(
                "sluice sent this pull request back: it no longer merges with what is ahead of \
                 it. `{}` conflicts with {}.\n\n{way}",
                paths.join("`, `"),
                if with.is_empty() {
                    branch.to_string()
                } else {
                    with.join(", ")
                }
            ))
        }
    }
}

/// The branch seam, each branch read once: an intake pass reads the branch
/// for each pull request it forecasts and submits, and one fetch serves
/// them all. Built for one pass, so no revision outlives it (rule core.14).
struct ReadOnce<'a> {
    branch: &'a dyn Branch,
    read: RefCell<BTreeMap<String, Option<Rev>>>,
}

impl Branch for ReadOnce<'_> {
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        if let Some(r) = self.read.borrow().get(branch) {
            return Ok(r.clone());
        }
        let r = self.branch.read(branch)?;
        self.read.borrow_mut().insert(branch.into(), r.clone());
        Ok(r)
    }

    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        self.branch.held(branch)
    }

    fn copy(&self, branch: &str) -> Result<HostCopy> {
        self.branch.copy(branch)
    }

    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        self.branch.level(branch, expected, to)
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        self.branch.read_file(rev, path)
    }

    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        self.branch.form(onto, head)
    }

    fn commit(&self, commit: &LandingCommit) -> Result<Rev> {
        self.branch.commit(commit)
    }

    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool> {
        self.branch.contains(tip, rev)
    }

    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        self.branch.landed_by(tip, rev)
    }

    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        self.branch.move_to(branch, expected, to)
    }

    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        self.branch.make(branch, at)
    }

    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        self.branch.remove(branch, expected)
    }
}

/// `seams` with the branch read once, for one pass.
fn once<'a>(seams: Seams<'a>, memo: &'a ReadOnce<'a>) -> Seams<'a> {
    Seams {
        branch: memo,
        ..seams
    }
}

/// 137 (design D25): `rows` with `lead`, the host that keeps their
/// branch; `None` where there are none.
fn across_of(lead: Option<String>, rows: Vec<AcrossRow>) -> Option<Across> {
    (!rows.is_empty()).then(|| Across {
        taken_in_by: lead.map(|host| TakenInBy { host }),
        pull_requests: rows,
    })
}

impl crate::PullRequests for Pulls {
    fn request_of(&self, place: &str) -> Option<u64> {
        number(place)
    }

    fn handed_over(&self, sub: &Submission) -> bool {
        handed_over(sub)
    }

    fn across(&self, seams: Seams<'_>, clock: &dyn Clock, branch: &str) -> Result<Option<Across>> {
        let s = Sluice::new(seams, clock);
        // Design D25: a branch on its remote that this host does not keep.
        let Some((t, lead)) = self.away_from(&s, branch)? else {
            return Ok(None);
        };
        let repo = &t.repo;
        let follows = self.kept.follows(repo, branch).requests;
        if follows.is_empty() {
            return Ok(None);
        }
        // A read that cannot be made is warned of, and the request shown as
        // the host last read it.
        let warn = |e: &anyhow::Error| crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
        let forge = self.forge(branch).map_err(|e| warn(&e)).ok();
        let now = clock.now();
        let read = |n: u64, kept: Follow| {
            match forge.map(|forge| self.read_followed(forge, repo, branch, n, now)) {
                Some(Ok(Some(f))) => f,
                // GitHub has no such pull request: followed no longer.
                Some(Ok(None)) => Follow {
                    open: false,
                    ..kept
                },
                Some(Err(e)) => {
                    warn(&e.context(format!("reading #{n} into {branch}")));
                    kept
                }
                None => kept,
            }
        };
        // 137: one read merged or closed is kept until a status or a wait
        // has said so, which lets it go; one the queue holds at its head
        // goes now.
        let (rows, _) = self.across_rows(&s, branch, follows, read)?;
        let held: Vec<u64> = self
            .kept
            .follows(repo, branch)
            .requests
            .keys()
            .copied()
            .filter(|n| !rows.iter().any(|r| r.pull_request == *n))
            .collect();
        self.unfollow(repo, branch, &held);
        Ok(across_of(lead, rows))
    }

    fn kept_across(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
    ) -> Result<Option<Across>> {
        // 129: the branch's policy read from git once while its revision
        // stands, across the reader's ticks.
        let s = Sluice::new(seams, clock).keeping(&self.policies);
        // Design D25: a branch on its remote that this host does not keep,
        // and the lead its records name, read with no process: the rows are
        // read again only once the kept follows or the lead moved (rule
        // core.31).
        let away = self.away_from(&s, branch)?;
        let Some(((t, lead), modified)) = away.and_then(|(t, lead)| {
            let modified = self.kept.follows_modified(&t.repo, branch)?;
            Some(((t, lead), modified))
        }) else {
            self.kept_across.borrow_mut().remove(branch);
            return Ok(None);
        };
        if let Some(k) = self.kept_across.borrow().get(branch) {
            if k.modified == modified && k.lead == lead {
                return Ok(k.across.clone());
            }
        }
        let follows = self.kept.follows(&t.repo, branch).requests;
        let (rows, gone) = self.across_rows(&s, branch, follows, |_, kept| kept)?;
        // 137 (design D3): what a status shows of one read merged or closed
        // says so once: it is followed no longer, and the next status shows
        // no row for it.
        self.unfollow(&t.repo, branch, &gone);
        let across = across_of(lead.clone(), rows);
        self.kept_across.borrow_mut().insert(
            branch.to_string(),
            KeptAcross {
                modified,
                lead,
                across: across.clone(),
            },
        );
        Ok(across)
    }

    fn branches(&self, seams: Seams<'_>) -> Result<Vec<String>> {
        let local = self.local()?;
        let Some((default, _)) = crate::profiles::default_branch(local)? else {
            return Ok(Vec::new());
        };
        // 129: at its revision as the host holds it, with no fetch, and
        // read from git once while that revision stands, its refusal in
        // the same words; a read that failed is made again.
        let Some(rev) = seams.branch.held(&default)? else {
            return Ok(Vec::new());
        };
        let kept = match &*self.default_policy.borrow() {
            Some((at, decided)) if *at == rev => Some(decided.clone()),
            _ => None,
        };
        let decided = match kept {
            Some(d) => d,
            None => {
                let decided = match seams.branch.read_file(&rev, POLICY_PATH)? {
                    None => Ok(None),
                    Some(bytes) => std::str::from_utf8(&bytes)
                        .map_err(anyhow::Error::from)
                        .and_then(Policy::parse)
                        .map(Some)
                        .map_err(|e| format!("{e:#}")),
                };
                *self.default_policy.borrow_mut() = Some((rev, decided.clone()));
                decided
            }
        };
        let Some(policy) = decided.map_err(|words| anyhow!(words))? else {
            return Ok(Vec::new());
        };
        let mut names = local.branch_names()?;
        names.extend(
            policy
                .branches
                .iter()
                .map(|(p, _)| p.clone())
                .filter(|p| !p.contains('*')),
        );
        names.sort();
        names.dedup();
        // A branch refused alone, one taking pull requests on records each
        // host keeps among them, is passed by: its round and its status say
        // its refusal (137; design D22).
        let mut out = Vec::new();
        for n in names {
            if let Ok(Some(r)) = policy.rules_for(&n) {
                if r.pull_requests.is_some() {
                    out.push(n);
                }
            }
        }
        Ok(out)
    }

    fn intake(&self, seams: Seams<'_>, clock: &dyn Clock, branch: &str) -> Result<crate::Intake> {
        let memo = ReadOnce {
            branch: seams.branch,
            read: RefCell::default(),
        };
        let s = Sluice::new(once(seams, &memo), clock)
            .keeping(&self.policies)
            .as_peer(self.peer.clone());
        self.causes.borrow_mut().clear();
        self.asked.set(false);
        let takes = self.take_in(&s, clock, branch);
        let causes = std::mem::take(&mut *self.causes.borrow_mut());
        Ok(crate::Intake {
            takes: takes?,
            asked: self.asked.get(),
            causes,
        })
    }

    fn settle(&self, seams: Seams<'_>, clock: &dyn Clock, branch: &str) -> Result<crate::Intake> {
        let memo = ReadOnce {
            branch: seams.branch,
            read: RefCell::default(),
        };
        let s = Sluice::new(once(seams, &memo), clock)
            .keeping(&self.policies)
            .as_peer(self.peer.clone());
        self.causes.borrow_mut().clear();
        let settled = self.settle_in(&s, branch);
        let causes = std::mem::take(&mut *self.causes.borrow_mut());
        settled?;
        Ok(crate::Intake {
            takes: true,
            asked: false,
            causes,
        })
    }

    fn elsewhere(&self, branch: &str) -> bool {
        self.away.borrow().contains_key(branch)
    }

    fn open(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        head: &SubmitHead,
        branch: &str,
        subject: Option<&str>,
    ) -> Result<Result<Opened, SubmitRefusal>> {
        Ok(
            match self.opening(seams, clock, std::slice::from_ref(head), branch, &[subject])? {
                Ok(mut opened) => Ok(opened.remove(0)),
                Err(GroupRefused::Worktrees(mut refused)) => Err(refused.remove(0).1),
                Err(GroupRefused::Short { .. }) => {
                    bail!("a submit of one head read its pull request's conditions")
                }
            },
        )
    }

    fn open_group(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        worktrees: &[String],
        subjects: &[String],
        branch: &str,
    ) -> Result<Result<Vec<Opened>, GroupRefused>> {
        // 125: one subject per worktree, in their order, or none.
        let subjects: Vec<Option<&str>> = if subjects.is_empty() {
            vec![None; worktrees.len()]
        } else if subjects.len() == worktrees.len() {
            subjects.iter().map(|s| Some(s.as_str())).collect()
        } else {
            bail!(
                "a group submit to {branch} gives {} subjects for {} worktrees",
                subjects.len(),
                worktrees.len()
            );
        };
        let heads: Vec<SubmitHead> = worktrees
            .iter()
            .cloned()
            .map(SubmitHead::Worktree)
            .collect();
        self.opening(seams, clock, &heads, branch, &subjects)
    }

    fn find(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
        worktree: &str,
    ) -> Result<Option<u64>> {
        let session = seams.driver.session(worktree)?;
        let s = Sluice::new(seams, clock);
        let Some(t) = self.target(&s, branch)? else {
            return Ok(None);
        };
        let forge = self.forge(branch)?;
        let head_ref = &session.from.worktree;
        if let Some(p) = forge
            .pulls(&t.repo, branch)?
            .into_iter()
            .find(|p| &p.head_ref == head_ref && p.from_repo(&t.repo))
        {
            return Ok(Some(p.number));
        }
        // Landed and closed: its entry has the head this worktree submits.
        let head = s.submitted_head(&session)?.head;
        Ok(s.history(branch)?
            .iter()
            .rev()
            .filter(|x| x.head == head)
            .find_map(|x| number(&x.from.worktree)))
    }

    fn follow(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        branch: &str,
        n: u64,
    ) -> Result<Followed> {
        let s = Sluice::new(seams, clock);
        let Some(t) = self.target(&s, branch)? else {
            bail!("{branch} takes no pull requests");
        };
        let forge = self.forge(branch)?;
        // 136: GitHub is asked about the request no more often than once
        // in a quarter of the host's interval; between, what it last said.
        let now = clock.now();
        // 137 (design D25): a branch another host keeps on its remote, as
        // its records name their lead, is followed by GitHub's word and
        // sluice's check on the request's head at the host's one pace,
        // naming that host; a wait runs no intake to find it so.
        match self.kept_elsewhere(&s, &t, branch)? {
            Some(keeper) => {
                self.away
                    .borrow_mut()
                    .insert(branch.to_string(), keeper.clone());
                return self.follow_across(forge, &t, branch, n, now, keeper.as_deref());
            }
            None => {
                self.away.borrow_mut().remove(branch);
            }
        }
        let fresh = self
            .seen
            .borrow()
            .get(&n)
            .filter(|seen| now - seen.at < self.discovery / 4)
            .map(|seen| seen.pull.clone());
        let pull = match fresh {
            Some(pull) => pull,
            None => {
                let pull = forge.pull(&t.repo, n)?;
                let seen = Seen {
                    at: now,
                    pull: pull.clone(),
                    readied: None,
                };
                self.seen.borrow_mut().insert(n, seen);
                pull
            }
        };
        let readied = || -> Result<Vec<DateTime<Utc>>> {
            if let Some(r) = self.seen.borrow().get(&n).and_then(|s| s.readied.clone()) {
                return Ok(r);
            }
            let r = forge.readied(&t.repo, n)?;
            if let Some(seen) = self.seen.borrow_mut().get_mut(&n) {
                seen.readied = Some(r.clone());
            }
            Ok(r)
        };
        let status = s.status(branch)?;
        let latest = self.latest_of(&s, branch, &[place(n)])?.remove(&place(n));
        if let Some(e) = &latest {
            let current = pull.as_ref().is_none_or(|p| p.head == e.head.0);
            if current && matches!(e.state, State::Landed | State::Failed | State::Conflict) {
                // Sent back on this head, and marked ready since: intake
                // takes it again, so that verdict is no longer its outcome.
                let again = match &pull {
                    Some(p) if p.open && e.state != State::Landed => {
                        let log = Line::read_log(&s.seams().state.read(branch)?)?;
                        match submits(p) {
                            Some(_) => readied_since(&readied()?, acts_from(&log, e)),
                            None => retried_by_hand(p, &log, e.id, readied)?,
                        }
                    }
                    _ => false,
                };
                if !again {
                    return Ok(Followed::Done(e.clone(), names(&status.submissions)));
                }
            }
        }
        if !pull.as_ref().is_some_and(|p| p.open) {
            // 122: closed as the branch took its head, whichever landing
            // took it: landed, as the commit that landed it. The branch is
            // read as a round reads it, from its remote, never from the
            // host's copy alone, which no fetch may have moved since another
            // queue's landing closed it; a read that fails is the follow's
            // error, never a closing (record 0100 D5; rule core.48).
            if let Some(e) = latest.filter(|e| e.state != State::Landed) {
                let branches = s.seams().branch;
                let to = match branches.read(branch)? {
                    Some(tip) => branches.landed_by(&tip, &e.head)?,
                    None => None,
                };
                if let Some(to) = to {
                    let landed = Submission {
                        state: State::Landed,
                        landed_as: Some(to),
                        landed_elsewhere: true,
                        ..e
                    };
                    return Ok(Followed::Done(landed, names(&status.submissions)));
                }
            }
            return Ok(Followed::Closed);
        }
        // Taken in on the head it has now, and undecided: the submission
        // a submit that does not wait answers with, and a wait follows.
        let submission = latest
            .filter(|e| !e.state.is_out() && pull.as_ref().is_some_and(|p| p.head == e.head.0));
        let names = names(&status.submissions);
        // As this process last showed it, else as the host last posted it
        // (136).
        let (title, lines) = match self.shown.borrow().get(&n) {
            Some(shown) => (shown.posting.title.clone(), shown.lines.clone()),
            None => {
                let posted = self
                    .kept
                    .readings(&t.repo, branch)
                    .requests
                    .remove(&n)
                    .filter(|r| pull.as_ref().is_some_and(|p| r.head == p.head))
                    .and_then(|mut r| r.posted.remove(SLUICE_CHECK));
                match posted {
                    Some(p) => (p.posting.title, Vec::new()),
                    None => ("waiting".into(), Vec::new()),
                }
            }
        };
        Ok(Followed::Open {
            title,
            lines,
            taken_in_by: None,
            queue: None,
            submission,
            names,
            head: pull.as_ref().map(|p| p.head.clone()),
            draft: pull.as_ref().is_some_and(|p| p.draft),
        })
    }
}

/// 137: where a pull request at `head` stands, read on a host that does
/// not take its branch's pull requests in, from `posting`, sluice's check
/// on its head, as [`told`] reads it, and its draft mark, never from the
/// check's status: GitHub keeps a check run completed once it was, and
/// takes only the words of a queued or in-progress posting after
/// (switchboard-kit #268; record 0096). Landed is landed, and failed or in
/// conflict sent back. Held for the next submit is sent back while the
/// pull request is a draft, which only a ready mark ends, the next
/// submit's or a person's; once one has, the request has had its
/// submitter's act and waits for `host`, which the branch's intake mark
/// names, to take it in again (120, C.5). Anything else is open as the
/// check says, with the queue it carries and its words beneath, the first
/// line, which says the same as the queue, left out (design D2, D5); no
/// check is open and `waiting`. Its `taken_in_by` is left for the caller,
/// which reads the branch's records for the host that keeps it.
fn as_its_check_says(
    posting: Option<Posting>,
    head: &str,
    draft: bool,
    host: Option<&str>,
) -> Followed {
    let open = |title: String, lines: Vec<String>, queue: Option<Queue>| Followed::Open {
        title,
        lines,
        taken_in_by: None,
        queue,
        submission: None,
        names: BTreeMap::new(),
        head: Some(head.to_string()),
        draft,
    };
    let Some(posting) = posting else {
        return open("waiting".into(), Vec::new(), None);
    };
    let queue = queue_of(&posting.summary);
    let sent_back = |conflict: bool, posting: Posting| Followed::SentBack {
        conflict,
        summary: words(&posting.summary),
        title: posting.title,
        head: head.to_string(),
        draft,
    };
    match told(Some(&posting)) {
        Told::Landed => landed_as_it_says(Some(posting), head, draft),
        Told::SentBack { conflict } => sent_back(conflict, posting),
        Told::Held if draft => sent_back(false, posting),
        Told::Held => open(
            match host {
                Some(host) => format!("waiting to be taken in by {host}"),
                None => "waiting to be taken in".into(),
            },
            Vec::new(),
            queue,
        ),
        Told::Open => {
            let said = words(&posting.summary);
            let first = queue.as_ref().map(first_line);
            let rest = first.and_then(|f| said.strip_prefix(&f)).unwrap_or(&said);
            let lines = rest
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect();
            open(posting.title, lines, queue)
        }
    }
}

/// 137: a pull request at `head` landed, while another host takes its
/// branch's pull requests in: titled and summed up as `posting`, sluice's
/// check on its head, says, where it says it landed, and `landed` with no
/// summary otherwise (124; record 0096).
fn landed_as_it_says(posting: Option<Posting>, head: &str, draft: bool) -> Followed {
    let (title, summary) = posting
        .filter(|p| p.title.starts_with("landed"))
        .map_or_else(
            || ("landed".into(), String::new()),
            |p| (p.title, words(&p.summary)),
        );
    Followed::Landed {
        title,
        summary,
        head: head.to_string(),
        draft,
    }
}

/// Whether `posting`, sluice's check on an open pull request at `head`,
/// says it was sent back, blamed or in conflict, or held for the next
/// submit, draft or not: [`as_its_check_says`] read as of a draft, whose
/// held row is sent back, so the reading names no host (120, C.5; record
/// 0096).
fn sent_back_or_held(posting: Option<Posting>, head: &str) -> bool {
    matches!(
        as_its_check_says(posting, head, true, None),
        Followed::SentBack { .. }
    )
}

impl Pulls {
    /// 137: where pull request `n` into `branch` stands while `host`
    /// takes its pull requests in: by GitHub's word and sluice's check on
    /// its head, which that host posts, as the host's kept follows hold
    /// them, read again only once a quarter of the host's interval has
    /// passed since any process of the host read them (136; design D4).
    /// Merged is landed, and closed and not merged is closed; otherwise it
    /// is as [`as_its_check_says`] reads the check, never as the conclusion
    /// GitHub keeps for it (record 0096). A request said to be landed or
    /// closed is followed no longer (design D3). One still open names
    /// `host`, the host that keeps the branch (design D25).
    fn follow_across(
        &self,
        forge: &dyn Forge,
        t: &Target,
        branch: &str,
        n: u64,
        now: DateTime<Utc>,
        host: Option<&str>,
    ) -> Result<Followed> {
        let repo = &t.repo;
        let Some(f) = self.read_followed(forge, repo, branch, n, now)? else {
            return Ok(Followed::Closed);
        };
        let posting = f.check.map(|p| p.posting);
        let mut followed = if f.merged {
            landed_as_it_says(posting, &f.head, f.draft)
        } else {
            match as_its_check_says(posting, &f.head, f.draft, host) {
                landed @ Followed::Landed { .. } => landed,
                _ if !f.open => Followed::Closed,
                other => other,
            }
        };
        match &mut followed {
            Followed::Landed { .. } | Followed::Closed => self.unfollow(repo, branch, &[n]),
            Followed::Open { taken_in_by, .. } => {
                *taken_in_by = host.map(|host| TakenInBy {
                    host: host.to_string(),
                });
            }
            _ => {}
        }
        Ok(followed)
    }

    /// 136, 137, design D4: pull request `n` into `branch` and sluice's
    /// check on its head as the host's kept follows hold them, where a
    /// process of the host read them within a quarter of its interval;
    /// otherwise read from GitHub, each read conditional on the answer the
    /// host keeps for its address, and kept for every process of the host.
    /// The process that finds the read due claims it under the follows'
    /// lock, so no two read it in one quarter, and gives the claim back
    /// where its read failed. `None`: GitHub has no such pull request, and
    /// it is followed no longer.
    fn read_followed(
        &self,
        forge: &dyn Forge,
        repo: &Repo,
        branch: &str,
        n: u64,
        now: DateTime<Utc>,
    ) -> Result<Option<Follow>> {
        let quarter = self.discovery / 4;
        let fresh = |f: &Follow| f.read_at.is_some_and(|at| now - at < quarter);
        let kept = self.kept.follows(repo, branch).requests.remove(&n);
        if let Some(f) = kept.filter(fresh) {
            return Ok(Some(f));
        }
        let claimed = self.kept.change_follows(repo, branch, |follows| {
            match follows.requests.get_mut(&n) {
                Some(f) if fresh(f) => Err(Box::new(f.clone())),
                Some(f) => Ok(f.read_at.replace(now)),
                None => Ok(None),
            }
        })?;
        let before = match claimed {
            Err(read) => return Ok(Some(*read)),
            Ok(before) => before,
        };
        let read = || -> Result<Option<Follow>> {
            let Some(p) = forge.pull(repo, n)? else {
                return Ok(None);
            };
            let check = match p.open || p.merged {
                true => forge.posted(repo, &p.head)?,
                false => None,
            };
            Ok(Some(Follow {
                read_at: Some(now),
                open: p.open,
                merged: p.merged,
                check,
                ..known(&p.head, p.draft, &p.title, &p.head_ref)
            }))
        };
        let read = match read() {
            Ok(read) => read,
            Err(e) => {
                let _ = self.kept.change_follows(repo, branch, |follows| {
                    if let Some(f) = follows.requests.get_mut(&n) {
                        if f.read_at == Some(now) {
                            f.read_at = before;
                        }
                    }
                });
                return Err(e);
            }
        };
        self.kept
            .change_follows(repo, branch, |follows| match &read {
                Some(f) => {
                    follows.requests.insert(n, f.clone());
                }
                None => {
                    follows.requests.remove(&n);
                }
            })?;
        Ok(read)
    }

    /// 137, design D3: follow each of `known` into `branch`, by number, as
    /// this host's own act on it left it: what was read of one at the same
    /// head stands, and the next process to follow one at a new head reads
    /// it. A follow that could not be kept is warned of, and the act stands.
    fn follow_known(&self, repo: &Repo, branch: &str, known: Vec<(u64, Follow)>) {
        let kept = self.kept.change_follows(repo, branch, |follows| {
            for (n, k) in known {
                match follows.requests.get_mut(&n) {
                    Some(f) if f.head == k.head => {
                        f.open = true;
                        f.draft = k.draft;
                        if !k.title.is_empty() {
                            f.title = k.title;
                        }
                        if !k.from.is_empty() {
                            f.from = k.from;
                        }
                    }
                    _ => {
                        follows.requests.insert(n, k);
                    }
                }
            }
        });
        if let Err(e) = kept {
            crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
    }

    /// 137, design D5: a row for each of `follows` into `branch` whose
    /// submission this host's queue does not hold at its head, as `read`
    /// gives it from what was kept of it, and the numbers to follow no
    /// longer: those the queue holds at their head, and those found merged
    /// or closed, which are shown `landed` or `closed without landing`.
    /// "Held" is the request's latest submission here at its
    /// head, in any state, past one this host handed over.
    fn across_rows(
        &self,
        s: &Sluice<'_>,
        branch: &str,
        follows: BTreeMap<u64, Follow>,
        mut read: impl FnMut(u64, Follow) -> Follow,
    ) -> Result<(Vec<AcrossRow>, Vec<u64>)> {
        let places: Vec<String> = follows.keys().map(|n| place(*n)).collect();
        let latest = self.latest_of(s, branch, &places)?;
        let held = |n: u64, head: &str| latest.get(&place(n)).is_some_and(|e| e.head.0 == head);
        let (mut rows, mut gone) = (Vec::new(), Vec::new());
        for (n, kept) in follows {
            // 137: one the branch's queue holds at its head is followed no
            // longer; the queue shows it.
            if held(n, &kept.head) {
                gone.push(n);
                continue;
            }
            let f = read(n, kept);
            if held(n, &f.head) {
                gone.push(n);
                continue;
            }
            // Design D6: the check's fields decide over its words.
            let posting = f.check.as_ref().map(|p| &p.posting);
            let state = if f.merged || told(posting) == Told::Landed {
                gone.push(n);
                crate::ACROSS_LANDED.to_string()
            } else if !f.open {
                gone.push(n);
                crate::ACROSS_CLOSED.to_string()
            } else {
                posting.map_or_else(|| "waiting".into(), |p| p.title.clone())
            };
            rows.push(AcrossRow {
                pull_request: n,
                queue: posting.and_then(|p| queue_of(&p.summary)),
                state,
                title: f.title,
                from: f.from,
                head: f.head,
                draft: f.draft,
                read_at: f.read_at,
            });
        }
        Ok((rows, gone))
    }

    /// 72, 137 (design D25): `branch`'s pull requests and where they are,
    /// where its records are on its remote and this host does not keep it,
    /// with the host its records name as keeping it, `None` where none
    /// does yet; `None` where the branch takes none, its records are each
    /// host's own, or this host keeps it.
    fn away_from(&self, s: &Sluice<'_>, branch: &str) -> Result<Option<(Target, Option<String>)>> {
        let Some(t) = self.target(s, branch)? else {
            return Ok(None);
        };
        Ok(self.kept_elsewhere(s, &t, branch)?.map(|lead| (t, lead)))
    }

    /// 137 (design D25): where `t`'s branch's records are on its remote
    /// and this host does not keep it, the host those records name as
    /// keeping it, as this host last fetched them, `None` where none does
    /// yet; `None` (the outer) where this host keeps it or each host keeps
    /// its own. Read with nothing written, so a wait may ask it.
    fn kept_elsewhere(
        &self,
        s: &Sluice<'_>,
        t: &Target,
        branch: &str,
    ) -> Result<Option<Option<String>>> {
        if !t.signed {
            return Ok(None);
        }
        let lead = s.lead_of(branch)?;
        Ok((lead.as_deref() != Some(self.peer.name.as_str())).then_some(lead))
    }

    /// 137, design D3: follow `numbers` into `branch` no longer, writing
    /// the kept follows only where they hold one of them; one that could
    /// not be let go is warned of.
    fn unfollow(&self, repo: &Repo, branch: &str, numbers: &[u64]) {
        let follows = self.kept.follows(repo, branch);
        if !numbers.iter().any(|n| follows.requests.contains_key(n)) {
            return;
        }
        let kept = self.kept.change_follows(repo, branch, |follows| {
            for n in numbers {
                follows.requests.remove(n);
            }
        });
        if let Err(e) = kept {
            crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
    }

    /// 125: every head read before anything is pushed, and a refusal for
    /// each refused; then, in order, each request marked one of the group
    /// before its head reaches it: an open request's description written
    /// for its head before its branch is pushed, a new one opened a draft
    /// with it once its branch is. Then the group is taken whole or left
    /// drafts ([`Pulls::take_act`]). One head is marked one of no group and
    /// marked ready, for intake to take on its conditions. Each head's of
    /// `subjects`, one per head, titles its request where given. A worktree
    /// is read through the driver's session; a branch named (124), through
    /// the core's, the branch as its remote holds it, so nothing of the
    /// folder the command runs in is read and nothing is pushed (record
    /// 0090; rule core.50).
    fn opening(
        &self,
        seams: Seams<'_>,
        clock: &dyn Clock,
        heads: &[SubmitHead],
        branch: &str,
        subjects: &[Option<&str>],
    ) -> Result<Result<Vec<Opened>, GroupRefused>> {
        let s = Sluice::new(seams, clock);
        let mut refused = Vec::new();
        // Each head read, by the name it was given, with the head a branch
        // named submits, read with it.
        let mut read: Vec<(&str, Session, Option<SubmittedHead>)> = Vec::new();
        for head in heads {
            let name = head.name();
            let (session, submitted) = match head {
                SubmitHead::Worktree(worktree) => (seams.driver.session(worktree)?, None),
                SubmitHead::Branch(named) => match s.named_session(named, branch)? {
                    Ok((session, submitted)) => (session, Some(submitted)),
                    Err(r) => {
                        refused.push((name.to_string(), r));
                        continue;
                    }
                },
            };
            let before = read
                .iter()
                .find(|(n, s, _)| *n == name || s.from.worktree == session.from.worktree);
            if let Some((earlier, _, _)) = before {
                refused.push((
                    name.to_string(),
                    SubmitRefusal::NamedTwice(earlier.to_string()),
                ));
            } else if session.detached {
                refused.push((name.to_string(), SubmitRefusal::Detached));
            } else if !session.dirty.is_empty() {
                refused.push((
                    name.to_string(),
                    SubmitRefusal::Dirty(session.dirty.clone()),
                ));
            }
            read.push((name, session, submitted));
        }
        let target = self.target(&s, branch)?;
        if target.is_none() {
            for h in heads {
                refused.push((h.name().to_string(), SubmitRefusal::NotManaged));
            }
        }
        let (Some(t), true) = (target, refused.is_empty()) else {
            return Ok(Err(GroupRefused::Worktrees(refused)));
        };
        // ADR 0050: refused before anything is pushed or opened, each place
        // its runner keeps a required check out of CI in; a branch named
        // has no place to ask, and is taken with nothing asked.
        let mut warnings = Vec::new();
        for (name, session, _) in &read {
            match s.check_kept_out(session, branch, &t.tip)? {
                Ok(w) => warnings.push(w),
                Err(r) => refused.push((name.to_string(), r)),
            }
        }
        if !refused.is_empty() {
            return Ok(Err(GroupRefused::Worktrees(refused)));
        }
        let forge = self.forge(branch)?;
        let _lock = self.wait_lock(branch)?;
        // 124, 1: a branch sluice lands on is asked for as its remote holds
        // it, and nothing is pushed; only a branch it lands on nowhere is.
        // A branch named was read so already.
        let submitted = read
            .iter()
            .map(|(_, session, named)| match named {
                Some(head) => Ok(head.clone()),
                None => s.submitted_head(session),
            })
            .collect::<Result<Vec<_>>>()?;
        let open = forge.pulls(&t.repo, branch)?;
        let n = read.len();
        let token = group_token(clock.now(), &submitted[0].head);
        let mut opened = Vec::new();
        // Each request as the act leaves it, and when its last submission
        // left the queue, for the group's taking.
        let mut acted = Vec::new();
        for (k, (((_, session, _), submitted), &subject)) in
            read.iter().zip(&submitted).zip(subjects).enumerate()
        {
            let head_ref = session.from.worktree.clone();
            let head = &submitted.head;
            let agent = &session.from.agent;
            let group = (n > 1).then(|| Group {
                token: token.clone(),
                k: k + 1,
                n,
                head: head.0.clone(),
            });
            let push = || -> Result<()> {
                if !submitted.from_remote {
                    self.pushed(&t.remote)?.push_branch(head, &head_ref)?;
                }
                Ok(())
            };
            let existing = open
                .iter()
                .find(|p| p.head_ref == head_ref && p.from_repo(&t.repo));
            // C.5: the description of the head submitted, its commits'
            // messages as many as GitHub holds; the head is in the
            // repository's objects before it is pushed.
            let messages = self.messages(&t.tip, head)?;
            let compare = compare_address(&t.repo, branch, &head_ref);
            let body = description(&messages, agent, group.as_ref(), &compare);
            let (pull, word) = match existing {
                Some(p) => {
                    // C.5 pull request, 120: a subject retitles a request
                    // sluice opened, never one a person opened; none leaves
                    // the title as it is.
                    let title = match subject {
                        Some(s) if s != p.title && p.author == forge.login(&t.repo)? => Some(s),
                        _ => None,
                    };
                    if p.body != body || title.is_some() {
                        forge.edit(&t.repo, p.number, title, &body)?;
                    }
                    push()?;
                    let title = title.map_or_else(|| p.title.clone(), String::from);
                    (
                        Pull {
                            body,
                            title,
                            ..p.clone()
                        },
                        "updated",
                    )
                }
                None => {
                    push()?;
                    // C.5 pull request: titled by what lands, the subject
                    // or the branch's name, never a commit's message.
                    let title = subject.unwrap_or(&head_ref);
                    // A group's request is opened a draft, which the act
                    // marks ready only once it takes the group (125).
                    let p = forge.open(&t.repo, branch, &head_ref, title, &body, n > 1)?;
                    (p, "opened")
                }
            };
            let number = pull.number;
            let mut said = format!("{word} #{number}: {}", pull.title);
            if !submitted.from_remote {
                said = format!("pushed {head_ref} · {said}");
            }
            // The submit is its submitter's act (120). The submission on an
            // older head leaves the queue, withdrawn by the submit itself
            // (67), and a ready mark follows the latest leaving, so intake
            // reads the act as given.
            let here = place(number);
            let latest = s.latest_of(branch, std::slice::from_ref(&here))?;
            if let Some(e) = latest
                .get(&here)
                .filter(|e| !e.state.is_out() && e.state != State::Landing && e.head != *head)
            {
                // Landing since it was read: it lands, and the new head waits.
                if let Ok(Withdrew::Now) =
                    s.withdraw_on(&e.branch, e.id, &Requester::Session(e.from.agent.clone()))?
                {
                    said.push_str(&format!(" · withdrew its submission {}", e.id));
                }
            }
            let latest = s
                .latest_of(branch, std::slice::from_ref(&here))?
                .remove(&here);
            let log = Line::read_log(&s.seams().state.read(branch)?)?;
            let left = latest.filter(left_unlanded).map(|e| acts_from(&log, &e));
            // 120, C.5: a submit takes again a request sent back or held for
            // it, draft or not, whichever host takes the branch's requests
            // in; where another does, this host's records hold no submission
            // of it that left the queue, and a request out of draft is held
            // there all the same (a request opened by hand, or one sent back
            // before that host's pass marked it a draft). So sluice's check
            // on the head it held before the push says, read once on the
            // submitter's own act; a new request, a draft or one whose
            // leaving this host's records give reads nothing (rule core.32;
            // record 0096). A check that says anything else leaves the
            // request as it is, never a draft even for a moment, since a
            // draft mark withdraws what the other host queued (C.5).
            let sent_back = match existing {
                Some(p) if n == 1 && !p.draft && left.is_none() => {
                    let posted = forge.posted(&t.repo, &p.head)?;
                    sent_back_or_held(posted.map(|p| p.posting), &p.head)
                }
                _ => false,
            };
            if n == 1 && (pull.draft || left.is_some() || sent_back) {
                mark_ready(forge, &t.repo, number, pull.draft, left)?;
                said.push_str(" · marked ready for review");
            }
            opened.push(Opened {
                number,
                said,
                copy: submitted.copy.clone(),
                submission: None,
            });
            acted.push((
                Pull {
                    head: head.0.clone(),
                    ..pull
                },
                left,
            ));
        }
        // 137 (design D25): where another host keeps the branch, as its
        // records name their lead, this host follows each request the act
        // opened, updated or marked ready.
        let away = match t.signed {
            true => s.lead_of(branch)?.is_some_and(|h| h != self.peer.name),
            false => false,
        };
        if away {
            let follows = acted
                .iter()
                .map(|(p, _)| {
                    (
                        p.number,
                        known(&p.head, n > 1 && p.draft, &p.title, &p.head_ref),
                    )
                })
                .collect();
            self.follow_known(&t.repo, branch, follows);
        }
        let taken = if n > 1 {
            let names: Vec<String> = heads.iter().map(|h| h.name().to_string()).collect();
            self.take_act(&s, forge, &t, branch, &names, &acted, opened)?
        } else {
            Ok(opened)
        };
        // 136: the host's next pass asks for the branch's pull requests at
        // once, the ones this act opened, updated or marked ready among them.
        if let Err(e) = self.kept.moved(&t.repo, branch) {
            crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
        // 124: how the host's copy differs, and ADR 0050: why the runner
        // could not be asked, beneath what the act did.
        let beneath = |mut opened: Vec<Opened>| {
            for (o, warnings) in opened.iter_mut().zip(&warnings) {
                if let Some(copy) = &o.copy {
                    o.said.push('\n');
                    o.said.push_str(&crate::copy_differs_line(copy));
                }
                for w in warnings {
                    o.said.push_str(&format!("\nwarning: {w}"));
                }
            }
            opened
        };
        Ok(match taken {
            Ok(opened) => Ok(beneath(opened)),
            Err(GroupRefused::Short { opened, short }) => Err(GroupRefused::Short {
                opened: beneath(opened),
                short,
            }),
            Err(refused) => Err(refused),
        })
    }

    /// 2, 47: the head of pull request `worktree`, `pull/<n>`, as intake
    /// read it this round, fetched from its remote; never dirty, never
    /// detached; its agent the one its body names, or its author; and the
    /// branch it was opened from, for a reader to find its worktree.
    pub fn session(&self, worktree: &str) -> Result<Session> {
        let k = self.known.borrow().get(worktree).cloned().ok_or_else(|| {
            anyhow!(
                "{worktree} is a pull request, which only intake submits, and it has not read it"
            )
        })?;
        let pushed = self.pushed(&k.remote)?;
        let head = self.fetch_head(&pushed, &k.pull)?;
        Ok(Session {
            head,
            from: Submitter {
                agent: marked_agent(&k.pull.body).unwrap_or_else(|| k.pull.author.clone()),
                worktree: worktree.into(),
                path: None,
                head: Some(k.pull.head_ref.clone()),
                host: None,
                session: None,
            },
            dirty: Vec::new(),
            detached: false,
        })
    }

    /// 50, 51: post the outcome on the pull request's head and, when it is
    /// sent back, comment once: delivered to the request once posted. An
    /// entry intake has not read this round holds no request here, and is
    /// not delivered; the next intake shows it, from its record.
    pub fn notify(&self, n: &Notification) -> Result<Delivery> {
        let Some(e) = self.entries.borrow().get(&n.submission).cloned() else {
            return Ok(Delivery::Undelivered {
                why: format!("no pull request is held for submission {}", n.submission),
            });
        };
        let to = place(e.number);
        let forge = self.forge(&n.branch)?;
        let names: BTreeMap<u64, String> = self
            .entries
            .borrow()
            .iter()
            .map(|(id, e)| (*id, format!("#{}", e.number)))
            .collect();
        let outcome = Outcome::decided(&n.decided);
        // D1: decided, so out of the queue, with nothing ahead of it.
        let queue = Queue {
            host: self.peer.name.clone(),
            taken_in: true,
            submission: Some(n.submission),
            state: Some(outcome.state().word().to_string()),
            ahead: Vec::new(),
            waits_for: Vec::new(),
        };
        let shown = outcome_shown(&n.branch, &outcome, &names, queue);
        // ADR 0044: a pull request that landed is closed, so no intake
        // reads it again: its report is posted with the outcome. What the
        // host keeps of the request says what is posted already (136).
        let log_lines = self.log_lines.borrow().get(&n.branch).copied().flatten();
        let reading = |f: &mut dyn FnMut(&mut Reading) -> Result<()>| -> Result<()> {
            self.kept.change_readings(&e.repo, &n.branch, |kept| {
                f(kept.requests.entry(e.number).or_default())
            })?
        };
        reading(&mut |r| {
            self.show_report(
                forge,
                &e.repo,
                e.number,
                &e.head,
                log_lines,
                &names,
                shown.clone(),
                n.report.as_ref(),
                r,
            )
        })?;
        if matches!(outcome, Outcome::Landed { .. }) {
            return Ok(Delivery::Handed {
                way: "pull-request".into(),
                to,
                confirms: true,
            });
        }
        // Its body, as intake read it this round, says whether a submit
        // opened it.
        let known = self
            .known
            .borrow()
            .get(&place(e.number))
            .map(|k| k.pull.clone());
        let pull = match known {
            Some(p) => Some(p),
            None => forge.pull(&e.repo, e.number)?,
        };
        let from = pull.as_ref().and_then(submits);
        if let Some(body) = comment(&n.branch, &outcome, &names, from) {
            reading(&mut |r| self.comment_once(forge, &e.repo, e.number, &n.id, &body, r))?;
        }
        Ok(Delivery::Handed {
            way: "pull-request".into(),
            to,
            confirms: true,
        })
    }
}

/// The file name of `branch`'s intake lock, before `.lock`: each `/`
/// written `__`, and one too long for a file name shortened. A branch
/// never holds `~`, which git forbids in a ref, so the mark is in no
/// kept name; 250 bytes and `.lock` are the 255 a file name may hold.
fn lock_name(branch: &str) -> String {
    crate::runs::shortened(branch, branch.replace('/', "__"), "~", 250)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::github::fake::FileForge;
    use crate::PullRequests as _;
    use sluice_core::verdict_view::{Row, RowResult};

    fn row(system: &str, replayed: bool) -> Row {
        Row {
            task: "cli:build".into(),
            system: Some(system.into()),
            key: Some("k".into()),
            result: RowResult::Passed,
            replayed,
            host: (!replayed).then(|| "box-1".into()),
            signer: Some(if replayed { "earlier" } else { "box" }.into()),
            executor: None,
            took_ms: Some(1_000),
            attempts: None,
            flaky: None,
            not_run: None,
            failed_by: Vec::new(),
            set_aside: None,
            log: None,
        }
    }

    /// pull-request-status, a build on three systems and a result read by
    /// key (many-hosts-3's C.5 addition; ADR 0044).
    #[test]
    fn a_check_per_required_check_per_system() {
        let dir = std::env::temp_dir().join(format!("sluice-pulls-checks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        let forge = Rc::new(FileForge::new(&dir.join("github.json"), &repo));
        let pulls = Pulls::new(
            &dir,
            &dir,
            Some(forge.clone()),
            None,
            Rc::new(retry::Policy::command()),
            Kept::new(&dir.join("state")),
            crate::instance::DISCOVERY,
            sluice_core::Peer::default(),
        );
        let mut reading = Reading::default();
        let view = VerdictView {
            branch: "main".into(),
            id: 12,
            standing: Stands::Testing,
            combination: Some(Rev::new("c1")),
            on: Some(Rev::new("m0")),
            submissions: vec![12],
            runs: Vec::new(),
            rows: vec![row("linux-x64", false), row("linux-arm64", true)],
            tasks: Vec::new(),
            pending: vec![("cli:build".into(), Some("macos-arm64".into()))],
        };
        let names = BTreeMap::from([(12, "#12".to_string())]);
        let shown = queued("main", &[], queue_at("b", None, &status_of(&[])));
        let mut post = |v: &VerdictView| {
            pulls
                .show_report(
                    &*forge,
                    &repo,
                    12,
                    "head",
                    None,
                    &names,
                    shown.clone(),
                    Some(v),
                    &mut reading,
                )
                .unwrap()
        };
        post(&view);
        let held = forge.held().unwrap();
        let by = |name: &str| {
            held.posted["head"]
                .iter()
                .find(|p| p.name == name)
                .map(|p| (p.posting.status, p.posting.title.clone()))
                .unwrap()
        };
        assert_eq!(
            by("sluice / cli:build (linux-x64)"),
            (Status::Success, "passed · on box-1 · signed by box".into())
        );
        assert_eq!(
            by("sluice / cli:build (linux-arm64)"),
            (
                Status::Success,
                "replayed · from a pass the gate trusts · signed by earlier".into()
            )
        );
        assert_eq!(by("sluice / cli:build (macos-arm64)").0, Status::InProgress);
        // Posted again unchanged: nothing.
        let posts = held.posts.len();
        post(&view);
        assert_eq!(forge.held().unwrap().posts.len(), posts);
        // Formed again: every check waits, none shows an outcome (20).
        let formed = VerdictView {
            standing: Stands::FormedAgain,
            rows: Vec::new(),
            pending: Vec::new(),
            ..view
        };
        post(&formed);
        // Record 0096 (D3): GitHub keeps a completed run completed, so a
        // check that waits again is read by its title, as any host reads
        // it now; what sluice sent for each is queued.
        let held = forge.held().unwrap();
        let per_check: Vec<&Posted> = held.posted["head"]
            .iter()
            .filter(|p| p.name != SLUICE_CHECK)
            .collect();
        assert_eq!(per_check.len(), 3);
        assert!(per_check.iter().all(|p| p.posting.title == "formed again"));
        assert!(per_check.iter().all(|p| held
            .posts
            .iter()
            .rfind(|(rev, name, _)| rev == "head" && *name == p.name)
            .is_some_and(|(_, _, sent)| sent.status == Status::Queued)));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 140, "A request's earlier report is brought up to date": a request
    /// whose earlier head was blamed, and whose new head lands beside the
    /// queue, has its one report comment edited to the landing beside's,
    /// which is also its `sluice` check's text, and no check per required
    /// check on its new head, since none was run or replayed there (118;
    /// record 0083).
    #[test]
    fn a_landing_beside_edits_the_requests_report() {
        let dir = std::env::temp_dir().join(format!("sluice-pulls-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        let forge = Rc::new(FileForge::new(&dir.join("github.json"), &repo));
        let pulls = Pulls::new(
            &dir,
            &dir,
            Some(forge.clone()),
            None,
            Rc::new(retry::Policy::command()),
            Kept::new(&dir.join("state")),
            crate::instance::DISCOVERY,
            sluice_core::Peer::default(),
        );
        let mut reading = Reading::default();
        let names = BTreeMap::from([(12, "#12".to_string())]);
        let mut failed = row("linux-x64", false);
        failed.result = RowResult::Failed;
        let blamed = VerdictView {
            branch: "main".into(),
            id: 12,
            standing: Stands::Blamed { timed_out: false },
            combination: Some(Rev::new("c1")),
            on: Some(Rev::new("m0")),
            submissions: vec![12],
            runs: Vec::new(),
            rows: vec![failed],
            tasks: Vec::new(),
            pending: Vec::new(),
        };
        let sent_back = outcome_shown(
            "main",
            &Outcome::Failed {
                tasks: vec!["cli:build".into()],
                logs: Vec::new(),
                combination: Rev::new("c1"),
                ids: vec![12],
                timed_out: false,
            },
            &names,
            queue_at("b", None, &status_of(&[])),
        );
        pulls
            .show_report(
                &*forge,
                &repo,
                12,
                "h1",
                None,
                &names,
                sent_back,
                Some(&blamed),
                &mut reading,
            )
            .unwrap();
        let held = forge.held().unwrap();
        let [first] = held.comments[&12].as_slice() else {
            panic!("one report comment: {:?}", held.comments);
        };
        assert!(
            first.body.contains("failed on main, #12 is to blame"),
            "{}",
            first.body
        );
        assert!(held.posted["h1"].iter().any(|p| p.name != SLUICE_CHECK));

        let covered_by = Some(CoveringVerdict {
            at: Rev::new("e25fea4c0ffee0000000000000000000000000ab"),
            result: "passed".into(),
            ran: 141,
            replayed: 0,
        });
        let beside = VerdictView {
            standing: Stands::Landed {
                landed_as: Rev::new("0cef58d1234"),
                beside: true,
                covered_by: covered_by.clone(),
            },
            combination: None,
            on: None,
            submissions: Vec::new(),
            rows: Vec::new(),
            ..blamed
        };
        let landed = outcome_shown(
            "main",
            &Outcome::Landed {
                landed_as: Rev::new("0cef58d1234"),
                beside: true,
                covered_by,
            },
            &names,
            queue_at("b", None, &status_of(&[])),
        );
        pulls
            .show_report(
                &*forge,
                &repo,
                12,
                "h2",
                None,
                &names,
                landed,
                Some(&beside),
                &mut reading,
            )
            .unwrap();
        let held = forge.held().unwrap();
        let [comment] = held.comments[&12].as_slice() else {
            panic!("still one report comment: {:?}", held.comments);
        };
        assert_eq!(comment.id, first.id);
        assert_eq!(held.edits.len(), 1);
        assert!(
            comment.body.contains(
                "## sluice: landed on main as 0cef58d with no check run: no required check \
                 reads its changes\n\nCovered by the result at \
                 [`e25fea4`](https://github.com/o/r/commit/e25fea4c0ffee0000000000000000000000000ab), \
                 passed (141 run, 0 replayed).\n"
            ),
            "{}",
            comment.body
        );
        let [check] = held.posted["h2"].as_slice() else {
            panic!(
                "only sluice's check on the new head: {:?}",
                held.posted["h2"]
            );
        };
        assert_eq!(check.name, SLUICE_CHECK);
        assert_eq!(check.posting.status, Status::Success);
        assert_eq!(check.posting.text.as_deref(), Some(comment.body.as_str()));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// reader-tick, "A refused policy stays refused at its revision", of
    /// the branches intake takes in: the default branch's policy sluice
    /// refuses is read from git once while its revision stands, each pass
    /// refused in the first's words, and read again once the branch moves;
    /// no policy file is kept the same way (129).
    #[test]
    fn a_refused_policy_is_kept_with_its_words() {
        let dir = std::env::temp_dir().join(format!("sluice-pulls-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        gix::init_bare(&dir).unwrap();
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let pulls = Pulls::new(
            &dir,
            &dir,
            None,
            None,
            Rc::new(retry::Policy::command()),
            Kept::new(&dir.join("state")),
            crate::instance::DISCOVERY,
            sluice_core::Peer::default(),
        );
        let fakes = sluice_fakes::Fakes::new();
        fakes
            .world
            .init("main", &[(POLICY_PATH, "[branches.\"main\"\n")]);
        let refused = || format!("{:#}", pulls.branches(fakes.seams()).unwrap_err());
        let words = refused();
        assert_eq!([refused(), refused()], [words.clone(), words]);
        assert_eq!(fakes.world.reads_of(POLICY_PATH), 1);
        // Moved: read again, and its having no policy file kept.
        fakes.world.hand_commit("main", &[(POLICY_PATH, None)]);
        for _ in 0..3 {
            assert!(pulls.branches(fakes.seams()).unwrap().is_empty());
        }
        assert_eq!(fakes.world.reads_of(POLICY_PATH), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A branch's lock keeps its name when short; a long one is shortened
    /// under the 255 bytes a file name may hold, and two long branches
    /// that differ only at the end keep two locks.
    #[test]
    fn a_long_branchs_lock_name_is_shortened() {
        assert_eq!(lock_name("bolt/concurrent-units"), "bolt__concurrent-units");
        let edge = "a".repeat(250);
        assert_eq!(lock_name(&edge), edge);
        let long = "bolt/".repeat(60);
        let (a, b) = (
            lock_name(&format!("{long}a")),
            lock_name(&format!("{long}b")),
        );
        assert!(a.len() + ".lock".len() <= 255, "{a}");
        assert!(a.starts_with("bolt__bolt__") && a.contains('~'), "{a}");
        assert_ne!(a, b);
        assert_eq!(a, lock_name(&format!("{long}a")));
    }

    #[test]
    fn places_and_markers() {
        assert_eq!(place(7), "pull/7");
        assert_eq!(number("pull/7"), Some(7));
        assert_eq!(number("unit/x"), None);
        assert_eq!(number("pull/x"), None);
        let body = with_marks("Adds review cards.", "crw-1", None);
        assert_eq!(marked_agent(&body).as_deref(), Some("crw-1"));
        let again = with_marks(&body, "crw-2", None);
        assert_eq!(marked_agent(&again).as_deref(), Some("crw-2"));
        assert_eq!(again.matches("sluice: agent=").count(), 1);
        assert!(again.starts_with("Adds review cards."));
        assert_eq!(marked_agent("by hand"), None);
    }

    /// 125: a group mark sits beside the agent mark, reads back as
    /// written, and a body written with no group drops it.
    #[test]
    fn a_group_mark_reads_back_and_is_dropped() {
        let g = Group {
            token: group_token(
                DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                &Rev::new("0123456789abcdef"),
            ),
            k: 2,
            n: 3,
            head: "0123456789abcdef".into(),
        };
        assert_eq!(
            g.marker(),
            "<!-- sluice: group=20261023T120000Z-0123456 2/3 head=0123456789abcdef -->"
        );
        let body = with_marks("Adds review cards.", "crw-1", Some(&g));
        assert_eq!(Group::of(&body), Some(g.clone()));
        assert_eq!(marked_agent(&body).as_deref(), Some("crw-1"));
        let again = with_marks(&body, "crw-1", Some(&g));
        assert_eq!(again.matches("sluice: group=").count(), 1);
        let single = with_marks(&body, "crw-1", None);
        assert_eq!(Group::of(&single), None);
        assert!(single.starts_with("Adds review cards."));
        assert_eq!(Group::of("<!-- sluice: group=t 4/3 head=x -->"), None);
    }

    const COMPARE: &str = "https://github.com/o/r/compare/main...unit/x";

    /// A request's marks with no group and with one.
    fn groups() -> [Option<Group>; 2] {
        [
            None,
            Some(Group {
                token: "20261024T120000Z-0123456".into(),
                k: 1,
                n: 2,
                head: "0123456789abcdef0123456789abcdef01234567".into(),
            }),
        ]
    }

    /// A message of `chars` characters: `subject`, a blank line, a body.
    fn message(subject: &str, chars: usize) -> String {
        let body = "x".repeat(chars - subject.chars().count() - 2);
        format!("{subject}\n\n{body}")
    }

    /// `body` no longer than GitHub holds, its marks read back.
    fn reads_back(body: &str, group: Option<&Group>) {
        let chars = body.chars().count();
        assert!(chars <= DESCRIPTION_LIMIT, "{chars} characters");
        assert_eq!(marked_agent(body).as_deref(), Some("crw-1"));
        assert_eq!(Group::of(body).as_ref(), group);
    }

    #[test]
    fn the_compare_view_of_a_request() {
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        assert_eq!(compare_address(&repo, "main", "unit/x"), COMPARE);
    }

    /// C.5 pull request: every message fits, so the description is each
    /// message whole, oldest first, then the marks.
    #[test]
    fn a_description_holds_every_message_that_fits() {
        let ms = vec![
            "Add the first file".to_string(),
            "Add the second file\n\nIts body.".to_string(),
        ];
        for g in groups() {
            let body = description(&ms, "crw-1", g.as_ref(), COMPARE);
            assert_eq!(body, with_marks(&ms.join("\n\n"), "crw-1", g.as_ref()));
            assert!(!body.contains("more commit"), "{body}");
            reads_back(&body, g.as_ref());
        }
    }

    /// C.5: three messages of 30,000 characters: two whole, the third
    /// named by its subject after the line counting it.
    #[test]
    fn a_description_too_long_names_the_rest_by_subject() {
        let ms: Vec<String> = (1..=3)
            .map(|k| message(&format!("Commit {k}"), 30_000))
            .collect();
        for g in groups() {
            let body = description(&ms, "crw-1", g.as_ref(), COMPARE);
            let text = format!(
                "{}\n\n{}\n\n1 more commit, each by its subject; all of them in full: \
                 {COMPARE}\n\n- Commit 3",
                ms[0], ms[1]
            );
            assert_eq!(body, with_marks(&text, "crw-1", g.as_ref()));
            reads_back(&body, g.as_ref());
        }
    }

    /// C.5: a message longer than the limit is never cut: its subject is
    /// named, and no part of its body is in the description.
    #[test]
    fn a_message_longer_than_the_limit_is_named_by_its_subject() {
        let ms = vec![message("One long commit", 70_000)];
        for g in groups() {
            let body = description(&ms, "crw-1", g.as_ref(), COMPARE);
            let text = format!(
                "1 more commit, each by its subject; all of them in full: \
                 {COMPARE}\n\n- One long commit"
            );
            assert_eq!(body, with_marks(&text, "crw-1", g.as_ref()));
            assert!(!body.contains("xx"), "{body}");
            reads_back(&body, g.as_ref());
        }
    }

    /// C.5: 70 subjects of 1,000 characters cannot all be named: none of
    /// the messages is whole, the subjects are named in order as many as
    /// fit, each whole, and the others counted.
    #[test]
    fn more_subjects_than_the_limit_holds_are_counted() {
        let ms: Vec<String> = (0..70)
            .map(|k| format!("{k:02}{}", "s".repeat(998)))
            .collect();
        for g in groups() {
            let body = description(&ms, "crw-1", g.as_ref(), COMPARE);
            let head =
                format!("70 more commits, each by its subject; all of them in full: {COMPARE}\n\n");
            assert!(body.starts_with(&head), "{}", &body[..200]);
            let named: Vec<&str> = body.lines().filter_map(|l| l.strip_prefix("- ")).collect();
            let j = named.len();
            assert!(j > 0 && j < 70, "{j} named");
            assert_eq!(
                named,
                ms[..j].iter().map(String::as_str).collect::<Vec<_>>()
            );
            assert!(
                body.contains(&format!("\nand {} more, in the compare view\n", 70 - j)),
                "{}",
                &body[body.len() - 300..]
            );
            // One more subject would not fit.
            assert!(body.chars().count() + 1_002 > DESCRIPTION_LIMIT);
            reads_back(&body, g.as_ref());
        }
    }

    /// C.5: the limit is counted in characters: a message of 39,000
    /// two-byte characters is held whole, though its bytes pass it.
    #[test]
    fn a_description_is_measured_in_characters() {
        let long = |k: usize| format!("Écrit le fichier {k}\n\n{}", "é".repeat(39_000));
        for g in groups() {
            let one = description(&[long(1)], "crw-1", g.as_ref(), COMPARE);
            assert!(one.len() > DESCRIPTION_LIMIT, "{} bytes", one.len());
            assert_eq!(one, with_marks(&long(1), "crw-1", g.as_ref()));
            reads_back(&one, g.as_ref());
            let two = description(&[long(1), long(2)], "crw-1", g.as_ref(), COMPARE);
            let text = format!(
                "{}\n\n1 more commit, each by its subject; all of them in full: \
                 {COMPARE}\n\n- Écrit le fichier 2",
                long(1)
            );
            assert_eq!(two, with_marks(&text, "crw-1", g.as_ref()));
            reads_back(&two, g.as_ref());
        }
    }

    /// A submission into `main` from `worktree`, in `state`.
    /// A branch's queue holding `subs`, its chain as a round orders it
    /// (6, 7), with no combination formed.
    fn status_of(subs: &[Submission]) -> sluice_core::Status {
        let landed = subs
            .iter()
            .filter(|s| s.state == State::Landed)
            .map(|s| s.id)
            .collect();
        sluice_core::Status {
            chain: sluice_core::order::chain(subs, &landed, &[], true).chain,
            submissions: subs.to_vec(),
            ..Default::default()
        }
    }

    fn submitted(id: u64, worktree: &str, state: &str) -> Submission {
        serde_json::from_value(serde_json::json!({
            "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": id, "branch": "main",
            "head": format!("h{id}"), "from": { "agent": "person", "worktree": worktree },
            "submitted": "2026-11-06T10:00:00Z", "after": [], "priority": 0, "attempt": 1,
            "state": state,
        }))
        .unwrap()
    }

    /// pull-request-status, waiting for a review: the check names the
    /// host that posts it and that the request waits to be taken in there,
    /// and its block what it waits for, in its title's words (D1).
    #[test]
    fn what_a_waiting_pull_request_shows() {
        let w = waiting(
            "main",
            None,
            &["1 review".into(), "qa/approved".into()],
            Some("forecast: merges onto main without conflict".into()),
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(w.posting.title, "waiting: needs 1 review, qa/approved");
        let s = &w.posting.summary;
        assert!(s.starts_with("Waiting to be taken in on b.\n\n"), "{s}");
        assert!(s.contains("1 review") && s.contains("`qa/approved`"), "{s}");
        assert!(
            s.ends_with(
                "<!-- sluice:queue {\"host\":\"b\",\"taken_in\":false,\"submission\":null,\
                 \"state\":null,\"ahead\":[],\"waits_for\":[\"1 review\",\"qa/approved\"]} -->"
            ),
            "{s}"
        );
        assert_eq!(w.lines, ["forecast: merges onto main without conflict"]);
        let draft = waiting(
            "main",
            None,
            &["a draft".into(), "1 review".into()],
            None,
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(draft.posting.title, "waiting: a draft");
        assert_eq!(
            queue_of(&draft.posting.summary).unwrap().waits_for,
            ["a draft"]
        );
        let held = waiting(
            "main",
            Some("unit/review"),
            &[NEXT_SUBMIT.into()],
            None,
            queue_at(
                "b",
                Some(&submitted(9, "pull/13", "failed")),
                &status_of(&[]),
            ),
        );
        let s = &held.posting.summary;
        assert!(s.starts_with("Submission 9 on b.\n\n"), "{s}");
        let q = queue_of(s).unwrap();
        assert_eq!(
            (q.taken_in, q.state.as_deref(), q.waits_for.as_slice()),
            (true, Some("failed"), [NEXT_SUBMIT.to_string()].as_slice())
        );
        let bound = Bound {
            n: 2,
            on: vec!["#14".into()],
            missing: Vec::new(),
        };
        let grouped = waiting_in_group(
            "main",
            None,
            &[],
            &bound,
            Some("forecast: merges onto main without conflict".into()),
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(grouped.posting.title, "waiting for its group: #14");
        let s = &grouped.posting.summary;
        assert!(s.starts_with("Waiting to be taken in on b.\n\n"), "{s}");
        assert!(s.contains("Forecast: merges onto main"), "{s}");
        assert_eq!(queue_of(s).unwrap().waits_for, ["its group: #14"]);
    }

    /// pull-request-status, queued behind another: the check names its
    /// submission on the host that posts it, and its block the
    /// submissions ahead of it there with their pull requests; testing and
    /// sent back name the same submission, with nothing ahead once it is
    /// out of the queue (D1).
    #[test]
    fn what_a_queued_pull_request_carries_its_queue() {
        let subs = vec![
            submitted(1404, "pull/9", "landed"),
            submitted(1405, "pull/10", "queued"),
            submitted(1406, "pull/12", "queued"),
        ];
        let names = names(&subs);
        let shown = queue_shown(
            "main",
            &subs[2],
            &status_of(&subs),
            &names,
            queue_at("b", Some(&subs[2]), &status_of(&subs)),
        );
        assert_eq!(shown.posting.title, "queued behind #10");
        let s = &shown.posting.summary;
        assert!(s.starts_with("Submission 1406 on b.\n\n"), "{s}");
        assert!(
            s.ends_with(
                "<!-- sluice:queue {\"host\":\"b\",\"taken_in\":true,\"submission\":1406,\
                 \"state\":\"queued\",\"ahead\":[{\"submission\":1405,\"pull_request\":10}],\
                 \"waits_for\":[]} -->"
            ),
            "{s}"
        );

        let landed = [subs[0].clone(), subs[2].clone()];
        let next = queue_shown(
            "main",
            &landed[1],
            &status_of(&landed),
            &names,
            queue_at("b", Some(&landed[1]), &status_of(&landed)),
        );
        assert_eq!(next.posting.title, "queued: next on main");
        assert!(queue_of(&next.posting.summary).unwrap().ahead.is_empty());

        let mut testing = subs[2].clone();
        testing.state = State::Testing;
        testing.tested = Some(sluice_core::records::Tested {
            combination: Rev::new("c1c1c1c1"),
            on: Rev::new("m0"),
            submissions: vec![1405, 1406],
        });
        let tested = queue_shown(
            "main",
            &testing,
            &status_of(&subs),
            &names,
            queue_at("b", Some(&testing), &status_of(&subs)),
        );
        assert_eq!(tested.posting.title, "testing main + #10 + #12");
        let q = queue_of(&tested.posting.summary).unwrap();
        assert_eq!(q.state.as_deref(), Some("testing"));
        assert_eq!(q.ahead.len(), 1);

        let mut failed = subs[2].clone();
        failed.state = State::Failed;
        let sent_back = outcome_shown(
            "main",
            &Outcome::Failed {
                tasks: vec!["cli:test".into()],
                logs: Vec::new(),
                combination: Rev::new("c1c1c1c1"),
                ids: vec![1405, 1406],
                timed_out: false,
            },
            &names,
            queue_at("b", Some(&failed), &status_of(&subs)),
        );
        let s = &sent_back.posting.summary;
        assert!(s.starts_with("Submission 1406 on b.\n\n"), "{s}");
        let q = queue_of(s).unwrap();
        assert_eq!(q.state.as_deref(), Some("failed"));
        assert!(q.ahead.is_empty() && q.waits_for.is_empty());

        let worktrees = vec![
            submitted(7, "/work/a", "queued"),
            submitted(8, "pull/12", "queued"),
        ];
        let q = queue_at("b", Some(&worktrees[1]), &status_of(&worktrees));
        assert_eq!(
            q.ahead,
            [Ahead {
                submission: 7,
                pull_request: None,
            }]
        );
    }

    /// pull-request-status, "each submission ahead of it": the block's
    /// `ahead` and the title's `queued behind` follow the queue's order,
    /// not the order of submission. A worktree's submission at priority 5
    /// is ahead of `#12`, submitted before it at 0; `#14`, at priority 9
    /// but after `#12`, is behind both; and a worktree's at priority -1,
    /// submitted first, is behind all three (6, 7; D1).
    #[test]
    fn the_check_names_whats_ahead_in_queue_order() {
        let mut subs = vec![
            submitted(1405, "/work/a", "queued"),
            submitted(1406, "pull/12", "queued"),
            submitted(1407, "/work/c", "queued"),
            submitted(1408, "pull/14", "queued"),
        ];
        subs[0].priority = -1;
        subs[2].priority = 5;
        subs[3].priority = 9;
        subs[3].after = vec![1406];
        let status = status_of(&subs);
        assert_eq!(status.chain, [1407, 1406, 1408, 1405]);
        let names = names(&subs);
        let ahead = |q: &Queue| -> Vec<(u64, Option<u64>)> {
            q.ahead
                .iter()
                .map(|a| (a.submission, a.pull_request))
                .collect()
        };

        let twelve = queue_at("b", Some(&subs[1]), &status);
        assert_eq!(ahead(&twelve), [(1407, None)]);
        let shown = queue_shown("main", &subs[1], &status, &names, twelve);
        assert_eq!(
            shown.posting.title,
            "queued behind submission 1407 (person)"
        );

        let fourteen = queue_at("b", Some(&subs[3]), &status);
        assert_eq!(ahead(&fourteen), [(1407, None), (1406, Some(12))]);
        let shown = queue_shown("main", &subs[3], &status, &names, fourteen.clone());
        assert_eq!(
            shown.posting.title,
            "queued behind submission 1407 (person), #12"
        );
        assert_eq!(queue_of(&shown.posting.summary), Some(fourteen));

        let first = queue_at("b", Some(&subs[0]), &status);
        assert_eq!(
            ahead(&first),
            [(1407, None), (1406, Some(12)), (1408, Some(14))]
        );

        // 7: one whose `after` is in the queue but not yet in the chain
        // has the chain ahead of it, then what it waits on.
        let mut waits = subs.clone();
        waits[3].after = vec![1409];
        waits.push(submitted(1409, "pull/15", "queued"));
        waits[4].after = vec![1];
        let status = status_of(&waits);
        assert_eq!(status.chain, [1407, 1406, 1405]);
        let fourteen = queue_at("b", Some(&waits[3]), &status);
        assert_eq!(
            ahead(&fourteen),
            [
                (1407, None),
                (1406, Some(12)),
                (1405, None),
                (1409, Some(15))
            ]
        );
    }

    /// D1: a reader takes the last block of a check's summary, and reads
    /// none in a check an older sluice posted, or in one whose block does
    /// not read; a `>` in a field never ends the comment; and the words a
    /// wait prints leave the block out.
    #[test]
    fn a_queue_block_reads_back_and_an_older_check_has_none() {
        let mut q = queue_at("b", None, &status_of(&[]));
        q.waits_for = vec!["ci/a->b".into()];
        let summary = summed(&q, "Needs `ci/a->b` on this commit before sluice takes it.");
        assert!(!summary[summary.rfind(QUEUE_BLOCK).unwrap()..]
            .trim_end_matches(" -->")
            .contains('>'));
        assert_eq!(queue_of(&summary), Some(q.clone()));
        assert_eq!(
            words(&summary),
            "Waiting to be taken in on b.\n\nNeeds `ci/a->b` on this commit before sluice takes it."
        );

        let older = "In main's queue; it lands once its combination passes.";
        assert_eq!(queue_of(older), None);
        assert_eq!(words(older), older);
        assert_eq!(queue_of("<!-- sluice:queue {\"host\": -->"), None);
        assert_eq!(queue_of("<!-- sluice:queue {\"host\":\"b\"}"), None);

        let mut later = q.clone();
        later.host = "c".into();
        let twice = format!("{summary}\n\n{}", summed(&later, "Again."));
        assert_eq!(queue_of(&twice), Some(later));
    }

    /// request-intake-host, "A pull request's state across hosts is read
    /// from its check's fields": a run GitHub keeps completed as a failure
    /// whose block reads `testing` is open, the block's `failed`,
    /// `conflict` and `landed` decide whatever the run's status, and its
    /// waiting for the next submit is held whatever its title; a check with
    /// no block is read by its title, never its status (D6; record 0096).
    #[test]
    fn the_fields_decide_over_the_words() {
        let posting = |status, title: &str, state: Option<&str>| {
            let mut q = queue_at("b", None, &status_of(&[]));
            if let Some(state) = state {
                q.taken_in = true;
                q.submission = Some(1406);
                q.state = Some(state.into());
            }
            let words = "In main's queue.";
            Posting {
                status,
                title: title.into(),
                summary: match state {
                    Some(_) => summed(&q, words),
                    None => words.into(),
                },
                text: None,
            }
        };
        let read = |status, title: &str, state| told(Some(&posting(status, title, state)));
        assert_eq!(
            read(Status::Failure, "testing main + #12", Some("testing")),
            Told::Open
        );
        assert_eq!(
            read(Status::InProgress, "failed: app:test", Some("failed")),
            Told::SentBack { conflict: false }
        );
        assert_eq!(
            read(Status::Failure, "failed: app:test", Some("conflict")),
            Told::SentBack { conflict: true }
        );
        assert_eq!(read(Status::Queued, "queued", Some("landed")), Told::Landed);
        let mut waiting = posting(Status::Failure, "waiting: the next submit", None);
        waiting.summary = summed(&queue_at("b", None, &status_of(&[])), "Held.");
        assert_eq!(told(Some(&waiting)), Told::Open);
        let mut held = queue_at("b", None, &status_of(&[]));
        held.taken_in = true;
        held.state = Some("failed".into());
        held.waits_for = vec![NEXT_SUBMIT.into()];
        waiting.title = "waiting: needs 1 review".into();
        waiting.summary = summed(&held, "Held.");
        assert_eq!(told(Some(&waiting)), Told::Held);

        assert_eq!(
            read(Status::Failure, "failed: app:test", None),
            Told::SentBack { conflict: false }
        );
        assert_eq!(
            read(Status::Failure, "conflict: plan.py", None),
            Told::SentBack { conflict: true }
        );
        assert_eq!(
            read(Status::Success, "waiting: the next submit", None),
            Told::Held
        );
        assert_eq!(read(Status::Failure, "conflict with #10", None), Told::Open);
        assert_eq!(read(Status::Success, "landed as abc", None), Told::Open);
        assert_eq!(told(None), Told::Open);
    }

    /// pull-request-status, a draft with no head: shown as a draft, with
    /// no forecast (128).
    #[test]
    fn a_draft_with_no_head_shows_no_forecast() {
        let w = waiting(
            "main",
            None,
            &["a draft".into()],
            None,
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(w.posting.title, "waiting: a draft");
        assert!(
            !w.posting.summary.contains("Forecast:"),
            "{}",
            w.posting.summary
        );
        assert!(w.lines.is_empty(), "{:?}", w.lines);
    }

    /// pull-request-status: a submit's pull request held for its
    /// submitter is queued, and names the next submit from its branch and
    /// the ready mark.
    #[test]
    fn what_a_held_pull_request_shows() {
        let w = waiting(
            "main",
            Some("unit/review"),
            &[NEXT_SUBMIT.into()],
            Some("forecast: merges onto main without conflict".into()),
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(w.posting.status, Status::Queued);
        assert_eq!(w.posting.title, "waiting: the next submit");
        let s = &w.posting.summary;
        assert!(
            s.contains("sluice marked it a draft")
                && s.contains("The next `sluice submit main` from `unit/review`")
                && s.contains("marking it ready for review"),
            "{s}"
        );
        assert!(s.contains("Forecast: merges onto main"), "{s}");
    }

    /// request-intake-host, records 0095 and 0096: across hosts a pull
    /// request reads as sluice's check on its head says, whatever status
    /// GitHub keeps for it: each posting sluice makes, with its own status
    /// and with the `Failure` GitHub kept from an earlier one, draft and
    /// not, read by its fields (D6) and, as an older sluice posted it with
    /// no block, by its title, where a landed check is open as it says.
    #[test]
    fn a_request_across_hosts_reads_as_its_check_says() {
        let names = BTreeMap::from([(12, "#12".to_string())]);
        let testing: Submission = serde_json::from_value(serde_json::json!({
            "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": 12, "branch": "main",
            "head": "h", "from": { "agent": "a", "worktree": "pull/12" },
            "submitted": "2026-11-06T22:00:00Z", "after": [], "priority": 0, "attempt": 2,
            "state": "testing",
            "tested": { "combination": "c1c1c1c1", "on": "m0m0m0m0", "submissions": [12] },
        }))
        .unwrap();
        let blamed = Outcome::Failed {
            tasks: vec!["sh check.sh".into()],
            logs: Vec::new(),
            combination: Rev::new("c1c1c1c1"),
            ids: vec![12],
            timed_out: false,
        };
        let timed_out = Outcome::Failed {
            tasks: Vec::new(),
            logs: Vec::new(),
            combination: Rev::new("c1c1c1c1"),
            ids: vec![12],
            timed_out: true,
        };
        let conflict = Outcome::Conflict {
            paths: vec!["plan.py".into()],
            with: vec!["#11".into()],
        };
        let landed = Outcome::Landed {
            landed_as: Rev::new("l1l1l1l1"),
            beside: false,
            covered_by: None,
        };
        // Its queue on b, the host that posts it, its submission 12 there
        // in `state`, or not taken in.
        let on_b = |state: Option<&str>| {
            let at = state.map(|st| submitted(12, "pull/12", st));
            queue_at("b", at.as_ref(), &status_of(at.as_slice()))
        };
        // What each reads as, draft and not: sent back (and whether a
        // conflict), landed, or open with the title a wait shows.
        #[derive(Clone, Debug, PartialEq)]
        enum Read {
            SentBack(bool, String),
            Landed(String),
            Open(String),
        }
        let held = waiting(
            "main",
            Some("unit/x"),
            &[NEXT_SUBMIT.into()],
            None,
            on_b(Some("failed")),
        );
        let cases: Vec<(Shown, Read, Read)> = vec![
            (
                held.clone(),
                Read::SentBack(false, "waiting: the next submit".into()),
                Read::Open("waiting to be taken in by b".into()),
            ),
            (
                waiting("main", None, &["1 review".into()], None, on_b(None)),
                Read::Open("waiting: needs 1 review".into()),
                Read::Open("waiting: needs 1 review".into()),
            ),
            (
                waiting("main", None, &["a draft".into()], None, on_b(None)),
                Read::Open("waiting: a draft".into()),
                Read::Open("waiting: a draft".into()),
            ),
            (
                queued("main", &[], on_b(Some("queued"))),
                Read::Open("queued: next on main".into()),
                Read::Open("queued: next on main".into()),
            ),
            (
                queued("main", &["#11".into()], on_b(Some("queued"))),
                Read::Open("queued behind #11".into()),
                Read::Open("queued behind #11".into()),
            ),
            (
                queue_shown(
                    "main",
                    &testing,
                    &status_of(std::slice::from_ref(&testing)),
                    &names,
                    on_b(Some("testing")),
                ),
                Read::Open("testing main + #12".into()),
                Read::Open("testing main + #12".into()),
            ),
            (
                outcome_shown("main", &blamed, &names, on_b(Some("failed"))),
                Read::SentBack(false, "failed: sh check.sh".into()),
                Read::SentBack(false, "failed: sh check.sh".into()),
            ),
            (
                outcome_shown("main", &timed_out, &names, on_b(Some("failed"))),
                Read::SentBack(false, "failed: timed out".into()),
                Read::SentBack(false, "failed: timed out".into()),
            ),
            (
                outcome_shown("main", &conflict, &names, on_b(Some("conflict"))),
                Read::SentBack(true, "conflict: plan.py".into()),
                Read::SentBack(true, "conflict: plan.py".into()),
            ),
            (
                outcome_shown("main", &landed, &names, on_b(Some("landed"))),
                Read::Landed("landed as l1l1l1l".into()),
                Read::Landed("landed as l1l1l1l".into()),
            ),
        ];
        let read = |posting: Option<Posting>, draft: bool| match as_its_check_says(
            posting.clone(),
            "h",
            draft,
            Some("b"),
        ) {
            Followed::SentBack {
                conflict,
                title,
                summary,
                head,
                draft: d,
            } => {
                let posting = posting.unwrap();
                assert_eq!(summary, words(&posting.summary), "{title}");
                assert_eq!((head.as_str(), d), ("h", draft), "{title}");
                Read::SentBack(conflict, title)
            }
            Followed::Landed {
                title,
                summary,
                head,
                draft: d,
            } => {
                let posting = posting.unwrap();
                assert_eq!(summary, words(&posting.summary), "{title}");
                assert_eq!((head.as_str(), d), ("h", draft), "{title}");
                Read::Landed(title)
            }
            Followed::Open {
                title,
                submission,
                head,
                draft: d,
                ..
            } => {
                assert!(submission.is_none(), "{title}");
                assert_eq!((head.as_deref(), d), (Some("h"), draft), "{title}");
                Read::Open(title)
            }
            _ => panic!("neither sent back, landed nor open"),
        };
        // As an older sluice posted it, with no block: read by its title,
        // where landed is open as it says.
        let older = |r: &Read| match r {
            Read::Landed(t) => Read::Open(t.clone()),
            r => r.clone(),
        };
        for (shown, as_draft, out_of_draft) in cases {
            let own = shown.posting.clone();
            assert!(queue_of(&own.summary).is_some(), "{}", own.title);
            let unblocked = Posting {
                summary: words(&own.summary),
                ..own.clone()
            };
            let forms = [
                (own, as_draft.clone(), out_of_draft.clone()),
                (unblocked, older(&as_draft), older(&out_of_draft)),
            ];
            for (form, as_draft, out_of_draft) in forms {
                let sticky = Posting {
                    status: Status::Failure,
                    ..form.clone()
                };
                for posting in [form, sticky] {
                    let status = posting.status;
                    let block = queue_of(&posting.summary).is_some();
                    // D4: a submit marks ready what reads sent back as a draft.
                    assert_eq!(
                        sent_back_or_held(Some(posting.clone()), "h"),
                        matches!(as_draft, Read::SentBack(..)),
                        "{status:?}, block {block}, a submit's reading"
                    );
                    assert_eq!(
                        read(Some(posting.clone()), true),
                        as_draft,
                        "{status:?}, block {block}, a draft"
                    );
                    assert_eq!(
                        read(Some(posting), false),
                        out_of_draft,
                        "{status:?}, block {block}, out of draft"
                    );
                }
            }
        }
        // No check: open, and waiting.
        for draft in [true, false] {
            assert_eq!(read(None, draft), Read::Open("waiting".into()));
        }
        assert!(!sent_back_or_held(None, "h"));
        // An open request's words beneath its state, as the check says
        // them, the first line naming its submission on b left out, and
        // the queue the check carries.
        match as_its_check_says(
            Some(queued("main", &[], on_b(Some("queued"))).posting),
            "h",
            false,
            Some("b"),
        ) {
            Followed::Open { lines, queue, .. } => {
                assert_eq!(
                    lines,
                    ["In main's queue; it lands once its combination passes."]
                );
                assert_eq!(queue.map(|q| q.submission), Some(Some(12)));
            }
            _ => panic!("not open"),
        }
    }

    /// pull-request-status, withdrawn by the operator: a hand-opened pull
    /// request someone other than its agent withdrew is queued, names who
    /// withdrew it, and says new commits or a ready mark after draft submit
    /// it again; intake's own withdrawal, as its agent, is no one else's.
    #[test]
    fn what_a_withdrawn_pull_request_shows() {
        let w = waiting(
            "main",
            None,
            &[format!("{WITHDRAWN_BY}{}", withdrawer("operator"))],
            Some("forecast: merges onto main without conflict".into()),
            queue_at("b", None, &status_of(&[])),
        );
        assert_eq!(w.posting.status, Status::Queued);
        assert_eq!(w.posting.title, "waiting: withdrawn by the operator");
        let s = &w.posting.summary;
        assert!(
            s.contains("Withdrawn from main's queue by the operator")
                && s.contains("New commits, or marking it a draft and then ready for review")
                && !s.contains("sluice marked it a draft"),
            "{s}"
        );
        assert!(s.contains("Forecast: merges onto main"), "{s}");
        assert_eq!(withdrawer("planner"), "the planner");
        assert_eq!(withdrawer("crw-2"), "crw-2");

        let sub = |by: &str| -> Submission {
            serde_json::from_value(serde_json::json!({
                "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": 4, "branch": "main",
                "head": "h", "from": { "agent": "person", "worktree": "pull/4" },
                "submitted": "2026-10-23T10:00:00Z", "after": [], "priority": 0, "attempt": 1,
                "state": "withdrawn",
                "withdrawn": { "by": by, "at": "2026-10-23T10:00:05Z" },
            }))
            .unwrap()
        };
        assert_eq!(withdrawn_by_another(&sub("operator")), Some("operator"));
        assert_eq!(withdrawn_by_another(&sub("planner")), Some("planner"));
        assert_eq!(withdrawn_by_another(&sub("person")), None);
    }

    /// pull-request-status, blamed: a submit's pull request is told it is
    /// a draft until the next submit or a ready mark; one opened by hand
    /// keeps today's words.
    #[test]
    fn the_comment_on_a_pull_request_sent_back() {
        let names = BTreeMap::from([(4, "#4".to_string())]);
        let failed = Outcome::Failed {
            tasks: vec!["sh check.sh".into()],
            logs: Vec::new(),
            combination: Rev::new("c1c1c1c1"),
            ids: vec![4],
            timed_out: false,
        };
        let conflict = Outcome::Conflict {
            paths: vec!["plan.py".into()],
            with: vec!["#3".into()],
        };
        for o in [&failed, &conflict] {
            let mine = comment("main", o, &names, Some("unit/review")).unwrap();
            assert!(
                mine.contains(
                    "sluice marked it a draft. The next `sluice submit main` from \
                     `unit/review`, or marking it ready for review, submits it again."
                ),
                "{mine}"
            );
            assert!(
                !mine.contains("Push a fix") && !mine.contains("and push"),
                "{mine}"
            );
            let by_hand = comment("main", o, &names, None).unwrap();
            assert!(
                by_hand.contains("draft and then ready for review to submit the same commits"),
                "{by_hand}"
            );
            assert!(!by_hand.contains("sluice marked it a draft"), "{by_hand}");
        }
        assert!(comment(
            "main",
            &Outcome::Landed {
                landed_as: Rev::new("l1"),
                beside: false,
                covered_by: None,
            },
            &names,
            Some("unit/review")
        )
        .is_none());
    }

    /// 140: the `sluice` check of a request landed beside the queue says
    /// so, that no check ran, and the result that covers it, as its
    /// landing commit names it (23; record 0083); an outcome recorded
    /// with none says what it said before.
    #[test]
    fn a_landing_beside_the_queue_says_no_check_ran() {
        let on_b = || {
            queue_at(
                "b",
                Some(&submitted(12, "pull/12", "landed")),
                &status_of(&[]),
            )
        };
        let covered = outcome_shown(
            "main",
            &Outcome::Landed {
                landed_as: Rev::new("0cef58d1234"),
                beside: true,
                covered_by: Some(CoveringVerdict {
                    at: Rev::new("e25fea4c0ffee0000000000000000000000000ab"),
                    result: "passed".into(),
                    ran: 141,
                    replayed: 0,
                }),
            },
            &BTreeMap::new(),
            on_b(),
        );
        assert_eq!(covered.posting.status, Status::Success);
        assert_eq!(covered.posting.title, "landed as 0cef58d beside the queue");
        assert_eq!(
            words(&covered.posting.summary),
            "Submission 12 on b.\n\nLanded on main as `0cef58d` beside the queue, with no check run: no required \
             check reads its changes. Covered by the result at `e25fea4`, passed (141 run, 0 \
             replayed)."
        );

        let shown = outcome_shown(
            "main",
            &Outcome::Landed {
                landed_as: Rev::new("l1abcdef0"),
                beside: true,
                covered_by: None,
            },
            &BTreeMap::new(),
            on_b(),
        );
        assert_eq!(shown.posting.title, "landed as l1abcde beside the queue");
        assert_eq!(
            words(&shown.posting.summary),
            "Submission 12 on b.\n\nLanded on main as `l1abcde` beside the queue, with no check run: no required \
             check reads its changes."
        );
    }

    /// A submit's ready mark: a draft first unless it is one, read back,
    /// and made again the next second while it reads before the second
    /// it counts from, three times at most, then refused naming both
    /// times.
    #[test]
    fn a_submits_ready_mark_is_read_back() {
        let dir = std::env::temp_dir().join(format!("sluice-pulls-ready-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        let f = FileForge::new(&dir.join("github.json"), &repo);
        let n = f
            .open_by("ann", "main", "unit/a", "aaa", false, None)
            .unwrap();
        let now = || Utc::now().trunc_subsecs(0);
        // Out of draft and nothing to count from: a draft, then ready.
        mark_ready(&f, &repo, n, false, None).unwrap();
        assert_eq!(f.readied(&repo, n).unwrap().len(), 1);
        assert!(!f.pull(&repo, n).unwrap().unwrap().draft);
        // A draft already: ready, and read back at the second it counts
        // from.
        f.set_draft(n, true).unwrap();
        mark_ready(&f, &repo, n, true, Some(now())).unwrap();
        assert_eq!(f.readied(&repo, n).unwrap().len(), 2);
        // GitHub a second behind: made again the next second.
        let next = now() + chrono::TimeDelta::seconds(1);
        mark_ready(&f, &repo, n, false, Some(next)).unwrap();
        let marks = f.readied(&repo, n).unwrap();
        assert!(marks.len() >= 4, "{marks:?}");
        assert!(marks.last().unwrap().trunc_subsecs(0) >= next, "{marks:?}");
        // Never at or after it: refused after three tries.
        let later = now() + chrono::TimeDelta::seconds(60);
        let before = f.readied(&repo, n).unwrap().len();
        let e = mark_ready(&f, &repo, n, false, Some(later))
            .unwrap_err()
            .to_string();
        assert_eq!(f.readied(&repo, n).unwrap().len(), before + 3);
        let at = |t: DateTime<Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert!(
            e.contains(&format!("counts from {}", at(later)))
                && e.contains("ready mark reads 20")
                && e.contains("after 3 tries"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The second a submission left the queue: its withdrawal, blame or
    /// conflict, whichever the log holds last for it.
    #[test]
    fn when_a_submission_left() {
        use sluice_core::records::LogLine;
        let line = |at: &str, event: Event| LogLine {
            schema: sluice_core::records::LOG_SCHEMA.into(),
            at: at.into(),
            event,
        };
        let log = [
            line(
                "2026-10-23T10:00:00Z",
                Event::Blamed {
                    id: 1,
                    combination: Rev::new("c"),
                },
            ),
            line(
                "2026-10-23T10:00:05Z",
                Event::Withdrawn {
                    id: 2,
                    by: "unit/a".into(),
                    handed_over_to: None,
                },
            ),
        ];
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(removal(&log, 1), Some(at("2026-10-23T10:00:00Z")));
        assert_eq!(removal(&log, 2), Some(at("2026-10-23T10:00:05Z")));
        assert_eq!(removal(&log, 3), None);
        // A ready mark counts from the second it left; one in the second
        // it was submitted in is the act that submitted it.
        let sub = |id: u64, submitted: &str| -> Submission {
            serde_json::from_value(serde_json::json!({
                "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": id, "branch": "main",
                "head": "h", "from": { "agent": "unit/a", "worktree": "pull/4" },
                "submitted": submitted, "after": [], "priority": 0, "attempt": 1,
                "state": "failed",
            }))
            .unwrap()
        };
        assert_eq!(
            acts_from(&log, &sub(2, "2026-10-23T09:59:00Z")),
            at("2026-10-23T10:00:05Z")
        );
        assert_eq!(
            acts_from(&log, &sub(1, "2026-10-23T10:00:00Z")),
            at("2026-10-23T10:00:01Z")
        );
        assert_eq!(
            acts_from(&log, &sub(3, "2026-10-23T10:01:00Z")),
            at("2026-10-23T10:01:01Z")
        );
    }

    /// A submission from pull request 4 into main, withdrawn as `withdrawn`
    /// says, its record as JSON.
    fn withdrawn(withdrawn: serde_json::Value) -> Submission {
        serde_json::from_value(serde_json::json!({
            "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": 7, "branch": "main",
            "head": "h", "from": { "agent": "unit/a", "worktree": "pull/4" },
            "submitted": "2026-11-06T21:00:00Z", "after": [], "priority": 0, "attempt": 1,
            "state": "withdrawn", "withdrawn": withdrawn,
        }))
        .unwrap()
    }

    /// 120, 137: a withdrawal for a hand-over of intake is told by the host
    /// its record names, whatever its words, and holds no request: it is
    /// no leaving, no one else's withdrawal, and a submit's pull request
    /// whose latest submission it is waits for no act and reads no timeline
    /// (record 0088 as amended, design D1).
    #[test]
    fn a_hand_overs_withdrawal_holds_no_request() {
        let e = withdrawn(serde_json::json!({
            "by": "anything at all", "at": "2026-11-06T21:17:42Z", "handed_over_to": "b",
        }));
        assert!(handed_over(&e));
        assert!(!left_unlanded(&e));
        assert_eq!(withdrawn_by_another(&e), None);
        let body = description(&[], "unit/a", None, "");
        let p = Pull {
            number: 4,
            node_id: "PR_4".into(),
            title: "unit/a".into(),
            body,
            author: "sluice".into(),
            draft: false,
            open: true,
            head: "h".into(),
            head_ref: "unit/a".into(),
            head_repo: "o/r".into(),
            base: "main".into(),
            updated_at: None,
            merged: false,
        };
        assert!(submits(&p).is_some());
        let held = is_held(&p, Some(&e), &[], || panic!("no timeline is read")).unwrap();
        assert!(!held);
        // A draft a person made after it is no hold of sluice's either.
        let draft = Pull {
            draft: true,
            ..p.clone()
        };
        assert!(!is_held(&draft, Some(&e), &[], || panic!("no timeline is read")).unwrap());
        // Any other withdrawal holds it as before: the operator's, its
        // agent's at a close, a resubmission's that a ready mark preceded.
        for by in ["operator", "planner", "unit/a"] {
            let e = withdrawn(serde_json::json!({ "by": by, "at": "2026-11-06T21:17:42Z" }));
            assert!(!handed_over(&e), "{by}");
            assert!(left_unlanded(&e), "{by}");
            assert!(
                is_held(&p, Some(&e), &[], || Ok(Vec::new())).unwrap(),
                "{by}"
            );
        }
    }

    /// 120, 137: a hand-over's withdrawal written by a sluice that named no
    /// host in its record, 0.60.0 to 0.70.2, is read as that sluice read it,
    /// by the words it gave every hand-over, and holds no request; any
    /// other record with no host named reads as it was (design D2).
    #[test]
    fn an_older_hand_overs_withdrawal_reads_as_it_was() {
        for by in [
            "b takes main's pull requests in",
            "another host takes main's pull requests in",
        ] {
            let e = withdrawn(serde_json::json!({ "by": by, "at": "2026-10-30T16:18:13Z" }));
            assert_eq!(e.withdrawn.as_ref().unwrap().handed_over_to, None);
            assert!(handed_over(&e), "{by}");
            assert!(!left_unlanded(&e), "{by}");
            assert_eq!(withdrawn_by_another(&e), None, "{by}");
        }
        // Another branch's words are no hand-over of this one's.
        let e = withdrawn(serde_json::json!({
            "by": "b takes bolt/x's pull requests in", "at": "2026-10-30T16:18:13Z",
        }));
        assert!(!handed_over(&e));
        // The operator's, written then, holds as it did.
        let e = withdrawn(serde_json::json!({ "by": "operator", "at": "2026-10-30T16:18:13Z" }));
        assert!(!handed_over(&e));
        assert!(left_unlanded(&e));
        assert_eq!(withdrawn_by_another(&e), Some("operator"));
        // A submission blamed is no hand-over whatever it says.
        let mut blamed =
            withdrawn(serde_json::json!({ "by": "x", "at": "t", "handed_over_to": "b" }));
        blamed.state = State::Failed;
        assert!(!handed_over(&blamed));
        assert!(left_unlanded(&blamed));
    }

    /// 120, S56, on GitHub's fake: blamed, then submitted again by a submit
    /// whose ready mark came before the core withdrew the blamed submission
    /// as it queued the new one. The resubmission's withdrawal is no leaving:
    /// the request is not held, so no pass marks it a draft and withdraws
    /// what the submit queued (#285 and #287, 2026-10-29).
    #[test]
    fn a_resubmissions_withdrawal_holds_no_ready_mark_back() {
        use sluice_core::records::LogLine;
        let dir = std::env::temp_dir().join(format!("sluice-pulls-resub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        let f = FileForge::new(&dir.join("github.json"), &repo);
        let body = description(&[], "unit/a", None, "");
        let n = f
            .open(&repo, "main", "unit/a", "A", &body, false)
            .unwrap()
            .number;
        let now = Utc::now().trunc_subsecs(0);
        let at = |t: DateTime<Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let line = |t: DateTime<Utc>, event: Event| LogLine {
            schema: sluice_core::records::LOG_SCHEMA.into(),
            at: at(t),
            event,
        };
        let blamed = now - chrono::TimeDelta::seconds(600);
        let mut log = vec![line(
            blamed,
            Event::Blamed {
                id: 1,
                combination: Rev::new("c"),
            },
        )];
        // The submit's ready mark, counted from when the blamed one left.
        mark_ready(&f, &repo, n, false, Some(blamed)).unwrap();
        // The core queues the new head seconds later, withdrawing the
        // blamed one as it does.
        log.push(line(
            now + chrono::TimeDelta::seconds(3),
            Event::Withdrawn {
                id: 1,
                by: RESUBMISSION.into(),
                handed_over_to: None,
            },
        ));
        let p = f.pull(&repo, n).unwrap().unwrap();
        assert!(!p.draft && submits(&p).is_some(), "{p:?}");
        let old: Submission = serde_json::from_value(serde_json::json!({
            "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": 1, "branch": "main",
            "head": p.head, "from": { "agent": "unit/a", "worktree": place(n) },
            "submitted": at(blamed - chrono::TimeDelta::seconds(600)), "after": [],
            "priority": 0, "attempt": 1, "state": "withdrawn",
            "withdrawn": { "by": RESUBMISSION, "at": at(now + chrono::TimeDelta::seconds(3)) },
        }))
        .unwrap();
        assert_eq!(acts_from(&log, &old), blamed);
        let held = is_held(&p, Some(&old), &log, || f.readied(&repo, n)).unwrap();
        assert!(!held, "the submit's ready mark is held back");
        // One that only a resubmission ever withdrew left then.
        let queued = [line(
            now,
            Event::Withdrawn {
                id: 2,
                by: RESUBMISSION.into(),
                handed_over_to: None,
            },
        )];
        assert_eq!(removal(&queued, 2), Some(now));
        let _ = std::fs::remove_dir_all(dir);
    }
}
