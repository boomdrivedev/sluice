//! The worktrunk driver profile (C.5; 2, 3, 47, 50, 51; ADR 0009;
//! `docs/plans/phase-1-integrations.md` section 7). Its front door:
//! worktrunk runs any `wt-<name>` on the path as `wt <name>`, so `wt
//! submit` runs `wt-submit`, which forwards to `sluice submit` from the
//! session's worktree. A submit records the herdr pane it ran in, and the
//! round that decides its outcome, on the same host, types it into that
//! pane as one line to the agent there ([`herdr`]); a waiting submit's
//! outcome is also its exit and its last lines. When a waiting submit
//! lands, `wt-submit` removes the session's worktree as
//! `wt merge` does (C.4): the driver's, never the core's (rule core.16).
//! Run again where the worktree's submission stands, at the revision
//! sluice says a submit from it would send (124), it submits nothing: it
//! follows that submission with `sluice wait`, removing the worktree on the
//! same terms once it lands, or under `--no-wait` answers with it. Its own
//! `--timeout` bounds a wait, a first submit's too, which submits without
//! waiting and then follows (C.5 worktrunk; record 0065): `sluice wait`
//! itself removes nothing.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context as _, Result};
use clap::{CommandFactory as _, FromArgMatches as _};
use sluice_seams::{Delivery, Driver, Notification, Rev, Session, Submitter};

pub mod herdr;

/// The worktrunk driver, on the host its peers name `host`.
pub struct Worktrunk {
    host: String,
}

impl Worktrunk {
    /// The driver on `host`, as its peers name it: the instance's
    /// `[peers] name`, else the hostname, which the binding reads (rule
    /// core.2).
    pub fn new(host: impl Into<String>) -> Worktrunk {
        Worktrunk { host: host.into() }
    }

    fn git(&self, worktree: &Path, args: &[&str]) -> Result<std::process::Output> {
        crate::workers::command("git", "git")
            .arg("-C")
            // A request's call reads a relative path from its caller's
            // folder (the change `sluice-host-makes-each-change`, design D1).
            .arg(crate::caller::path(worktree))
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .stdin(Stdio::null())
            .output()
            .context("running git")
    }

    /// 2: whether `dir` has files checked out: false for a bare repository
    /// or a git directory, where git status refuses to run.
    fn checked_out(&self, dir: &Path) -> Result<bool> {
        let inside = self.git(dir, &["rev-parse", "--is-inside-work-tree"])?;
        Ok(String::from_utf8_lossy(&inside.stdout).trim() == "true")
    }
}

/// `git status --porcelain=v1 -z --untracked-files=all`, one entry per
/// path held beyond the commits, as git shows it: `XY path`, and for a
/// rename or copy `XY new <- old`.
pub fn dirty(status: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut fields = status.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let Some(f) = fields.next() {
        let text = String::from_utf8_lossy(f).into_owned();
        let moved = matches!(text.as_bytes().first(), Some(b'R' | b'C'))
            || matches!(text.as_bytes().get(1), Some(b'R' | b'C'));
        if moved {
            if let Some(old) = fields.next() {
                out.push(format!("{text} <- {}", String::from_utf8_lossy(old)));
                continue;
            }
        }
        out.push(text);
    }
    out
}

/// Who submitted: the session's own name when its harness sets one
/// (`SLUICE_AGENT`), else `branch` (3).
fn agent_or(branch: &str) -> String {
    crate::caller::var("SLUICE_AGENT")
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| branch.to_string())
}

impl Driver for Worktrunk {
    fn session(&self, worktree: &str) -> Result<Session> {
        let dir = Path::new(worktree);
        let head = self.git(dir, &["rev-parse", "--verify", "HEAD"])?;
        if !head.status.success() {
            bail!(
                "{worktree} is no git worktree with a commit: {}",
                String::from_utf8_lossy(&head.stderr).trim()
            );
        }
        let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        let branch = self.git(dir, &["symbolic-ref", "-q", "--short", "HEAD"])?;
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
        // 2: a repository with no files checked out, a bare one or a git
        // directory, holds no uncommitted work, and git status refuses to
        // run there; nor has it a top level to find a session by (record
        // 0085).
        let (path, dirty) = if !self.checked_out(dir)? {
            (None, Vec::new())
        } else {
            // Where it was submitted from, for a reader to find the session
            // by its worktree.
            let top = self.git(dir, &["rev-parse", "--show-toplevel"])?;
            let path = String::from_utf8_lossy(&top.stdout).trim().to_string();
            let status = self.git(
                dir,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )?;
            if !status.status.success() {
                bail!(
                    "git status in {worktree}: {}",
                    String::from_utf8_lossy(&status.stderr).trim()
                );
            }
            let path = (top.status.success() && !path.is_empty()).then_some(path);
            (path, dirty(&status.stdout))
        };
        let agent = agent_or(&branch);
        Ok(Session {
            head: Rev::new(head),
            from: Submitter {
                agent,
                worktree: branch.clone(),
                path,
                head: None,
                // 51: where the outcome is pushed once it is decided.
                host: Some(self.host.clone()),
                session: herdr::session_here(),
            },
            dirty,
            detached: branch.is_empty(),
        })
    }

    /// 51: who collects outcomes in `folder`: the branch its `HEAD` names,
    /// read with `symbolic-ref`, so a branch with no commit yet is read all
    /// the same; the top level as its path only where files are checked
    /// out; the agent the session's harness names, else the branch; this
    /// host; the herdr pane it runs in. No `HEAD` commit and no `git status`
    /// are read, so a bare repository or a git directory is the worktree on
    /// its branch with no path (2, 3; record 0099; rule core.48). `None`
    /// where `HEAD` names no branch.
    fn recipient(&self, folder: &str) -> Result<Option<Submitter>> {
        let dir = Path::new(folder);
        let read = self.git(dir, &["symbolic-ref", "-q", "--short", "HEAD"])?;
        // `-q` answers 1, saying nothing, for a `HEAD` on no branch; any
        // other failure is a folder git cannot read as a repository.
        if !read.status.success() && read.status.code() != Some(1) {
            bail!(
                "{folder} is no git repository: {}",
                String::from_utf8_lossy(&read.stderr).trim()
            );
        }
        let branch = String::from_utf8_lossy(&read.stdout).trim().to_string();
        if branch.is_empty() {
            return Ok(None);
        }
        let path = if self.checked_out(dir)? {
            let top = self.git(dir, &["rev-parse", "--show-toplevel"])?;
            let path = String::from_utf8_lossy(&top.stdout).trim().to_string();
            (top.status.success() && !path.is_empty()).then_some(path)
        } else {
            None
        };
        Ok(Some(Submitter {
            agent: agent_or(&branch),
            worktree: branch,
            path,
            head: None,
            host: Some(self.host.clone()),
            session: herdr::session_here(),
        }))
    }

    /// 124: a branch submitted by name, from anywhere in the repository:
    /// the agent the session's harness names, else the branch; the branch
    /// as its work and its head; this host. No path, no pane, and nothing
    /// read of the folder the command runs in, so no `git status` (rule
    /// core.48): its outcome is pushed into no session (record 0090).
    fn named(&self, branch: &str) -> Result<Submitter> {
        Ok(Submitter {
            agent: agent_or(branch),
            worktree: branch.into(),
            path: None,
            head: Some(branch.into()),
            host: Some(self.host.clone()),
            session: None,
        })
    }

    /// 50, 51: push the outcome into the pane the submit ran in, ending with
    /// the key it is confirmed by: a hand-over by `notice` to the pane once
    /// herdr takes it, which confirms nothing. A submit in no pane, or on
    /// another host, is not handed over, and nothing is called; herdr's
    /// refusal is not handed over, in its words (record 0102; design D5,
    /// D10).
    fn notify(&self, n: &Notification) -> Result<Delivery> {
        let undelivered = |why: String| Ok(Delivery::Undelivered { why });
        let Some(session) = &n.to.session else {
            return undelivered("no session recorded".into());
        };
        if n.to.host.as_deref() != Some(self.host.as_str()) {
            return undelivered(format!(
                "{session} is on {}, and this round on {}",
                n.to.host.as_deref().unwrap_or("a host it did not name"),
                self.host
            ));
        }
        let Some((pane, socket)) = herdr::pane_of(session) else {
            return undelivered(format!("{session} is no herdr pane"));
        };
        // 51, D10: the notice, then the key the agent confirms it by.
        let notice = format!(
            "{} sluice confirm {} once you have read it.",
            sluice_core::notice(n.submission, &n.branch, &n.decided),
            n.id
        );
        Ok(match herdr::prompt(pane, socket, &notice, herdr::BOUND)? {
            herdr::Answer::Took => Delivery::Handed {
                way: "notice".into(),
                to: session.clone(),
                confirms: false,
            },
            herdr::Answer::Refused(why) => Delivery::Undelivered { why },
        })
    }
}

/// The `sluice` command line a `wt submit` stands for.
pub fn forward(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut out = vec![OsString::from("submit")];
    out.extend(args);
    out
}

/// What `wt submit` does: the arguments for sluice, and the submit from
/// one worktree, if it is one; or why its command line is refused (64)
/// with nothing run.
#[derive(Debug, PartialEq, Eq)]
pub struct Submit {
    pub sluice: Vec<OsString>,
    pub single: Option<Single>,
    pub refused: Option<String>,
}

/// A submit from one worktree, waiting or not, which reads what stands
/// before it submits: the branch it submits to and the worktree it is
/// from.
#[derive(Debug, PartialEq, Eq)]
pub struct Single {
    pub branch: String,
    pub worktree: PathBuf,
    /// No `--no-wait`: it waits for the outcome, and so may remove.
    pub waits: bool,
    /// Its worktree goes once it lands: no `--no-remove`.
    pub remove: bool,
    /// `wt submit`'s own `--timeout`, as given: how long a waiting run
    /// follows its submission, standing or just made.
    pub timeout: Option<OsString>,
    /// `--json`, for the wait it follows with and the status it answers
    /// with.
    pub json: bool,
    /// `--after` or `--priority` given: a place in the queue, which only a
    /// new submission takes.
    pub orders: bool,
}

impl Submit {
    /// `--no-remove` and `--timeout` are `wt submit`'s own; the rest is
    /// read as sluice reads it, so that only a submit from one worktree
    /// reads what stands, follows or removes anything: not one that
    /// submits nothing (`--check`, `--status`), a branch by name (124) or
    /// several worktrees as one act (125), and not `--help`, `--version` or arguments sluice
    /// refuses, which exit as they do with nothing submitted. `--timeout`
    /// bounds a wait, so it is refused on a run that does not wait.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Submit {
        let mut keep = false;
        let mut timeout: Option<OsString> = None;
        let mut refused = None;
        let mut rest = Vec::new();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            match a.to_str() {
                Some("--no-remove") => keep = true,
                Some("--timeout") => match args.next() {
                    Some(t) => timeout = Some(t),
                    None => refused = Some("--timeout takes a duration, as 9m".to_string()),
                },
                Some(t) if t.starts_with("--timeout=") => {
                    timeout = Some(t["--timeout=".len()..].into());
                }
                _ => rest.push(a),
            }
        }
        if let (None, Some(t)) = (&refused, &timeout) {
            let text = t.to_string_lossy();
            if let Err(e) = crate::timeout(&text) {
                refused = Some(format!("--timeout {text}: {e}"));
            }
        }
        let sluice = forward(rest);
        let read = crate::Cli::command()
            .try_get_matches_from(
                std::iter::once(OsString::from("sluice")).chain(sluice.iter().cloned()),
            )
            .ok()
            .and_then(|m| {
                let given = |id: &str| {
                    m.subcommand_matches("submit")
                        .and_then(|s| s.value_source(id))
                        == Some(clap::parser::ValueSource::CommandLine)
                };
                let orders = given("after") || given("priority");
                crate::Cli::from_arg_matches(&m)
                    .ok()
                    .and_then(|c| c.command)
                    .map(|c| (c, orders))
            });
        let Some((
            crate::Command::Submit {
                no_wait,
                check,
                head,
                status,
                worktree,
                json,
                branch,
                ..
            },
            orders,
        )) = read
        else {
            // Help, a version or a command line sluice refuses: sluice says
            // so, as it would with no `--timeout`.
            return Submit {
                sluice,
                single: None,
                refused,
            };
        };
        if refused.is_none() && timeout.is_some() {
            let given = if no_wait {
                Some("this one gives --no-wait")
            } else if check {
                Some("this one gives --check")
            } else if status {
                Some("this one gives --status")
            } else if head.is_some() {
                Some("this one submits a branch by name")
            } else if worktree.len() > 1 {
                Some("this one submits several worktrees")
            } else {
                None
            };
            if let Some(given) = given {
                refused = Some(format!("--timeout is for a run that waits, and {given}"));
            }
        }
        // Several worktrees are one act of `sluice submit`'s, which none
        // follows and none is removed by; a branch submitted by name is no
        // worktree's (124).
        let single = (!check && !status && head.is_none() && worktree.len() <= 1).then(|| Single {
            branch,
            worktree: worktree.first().map_or(".", String::as_str).into(),
            waits: !no_wait,
            remove: !keep,
            timeout,
            json,
            orders,
        });
        Submit {
            sluice,
            single,
            refused,
        }
    }
}

/// What stands of a worktree's last submission to a branch, at the
/// revision a submit from the worktree would send now (124; record 0065).
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// A submission of that revision, undecided or landed.
    Submission { id: u64, landed: bool },
    /// The worktree's pull request at that revision: open and ready, not
    /// yet decided, or `landed` there, on whichever host takes the
    /// branch's pull requests in (137; record 0096).
    PullRequest { number: u64, landed: bool },
    /// None: never submitted, or of another revision, or blamed,
    /// conflicting, withdrawn or closed, a pull request sent back on any
    /// host among them (67, 120; record 0096).
    Nothing,
}

/// A submission's states that stand: undecided, or landed.
const STANDS: [&str; 6] = [
    "queued", "waiting", "testing", "passed", "landing", "landed",
];

/// [`Standing`] read from `sluice submit <branch> --status --json`'s exit
/// `code` and output, against the revision it says a submit would send,
/// its `sends`; or, when sluice gave no answer to read, its words. The
/// driver never decides that revision itself (rule core.27).
pub fn standing(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> std::result::Result<Standing, String> {
    let words = || {
        let said = String::from_utf8_lossy(stderr).trim().to_string();
        let said = if said.is_empty() {
            String::from_utf8_lossy(stdout).trim().to_string()
        } else {
            said
        };
        match code {
            Some(c) => format!("sluice submit --status exited {c}: {said}"),
            None => format!("sluice submit --status was stopped: {said}"),
        }
    };
    // 0 says where it stands; 1 that nothing was submitted, or a pull
    // request closed.
    if !matches!(code, Some(0 | 1)) {
        return Err(words());
    }
    let Ok(doc) = serde_json::from_slice::<serde_json::Value>(stdout) else {
        return Err(words());
    };
    if let Some(error) = doc.get("error") {
        return match error["kind"].as_str() {
            Some("not_found") => Ok(Standing::Nothing),
            _ => Err(error["message"].as_str().map_or_else(words, str::to_string)),
        };
    }
    let state = doc["state"].as_str().unwrap_or_default();
    let pull = doc["pull_request"].as_u64().filter(|_| doc["id"].is_null());
    // Closed, it stands at no revision.
    if pull.is_some() && state == "closed" {
        return Ok(Standing::Nothing);
    }
    let Some(sends) = doc["sends"].as_str() else {
        return Err(words());
    };
    // Sent back, failed or in conflict, on whichever host took it in,
    // it stands at no revision, as a failed submission does, and the
    // worktree submits it again (67, 120; record 0096).
    if pull.is_some() && matches!(state, "failed" | "conflict") {
        return Ok(Standing::Nothing);
    }
    let at = doc["head"] == sends;
    if let Some(id) = doc["id"].as_u64() {
        return Ok(if at && STANDS.contains(&state) {
            Standing::Submission {
                id,
                landed: state == "landed",
            }
        } else {
            Standing::Nothing
        });
    }
    if let Some(number) = pull {
        // Landed on another host, it stands where it landed, draft or not
        // (137; record 0096).
        let landed = state == "landed";
        let ready = doc["draft"] == false;
        return Ok(if at && (landed || ready) {
            Standing::PullRequest { number, landed }
        } else {
            Standing::Nothing
        });
    }
    Err(words())
}

/// What stands of `worktree`'s last submission to `branch`, as sluice
/// records it, at the revision a submit from the worktree would send now;
/// or why it could not be read.
pub fn standing_in(branch: &str, worktree: &Path) -> std::result::Result<Standing, String> {
    let out = Command::new("sluice")
        .args(["submit", branch, "--status", "--json", "--worktree"])
        .arg(worktree)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("running sluice: {e}"))?;
    standing(out.status.code(), &out.stdout, &out.stderr)
}

/// Whether sluice records `worktree`'s submission to `branch` as landed:
/// its last submission from the worktree, or its pull request another
/// host took in (137; record 0096), has landed, and is of the revision a
/// submit from the worktree would send now. A worktree is removed on
/// this, never on an exit alone.
pub fn landed(branch: &str, worktree: &Path) -> bool {
    matches!(
        standing_in(branch, worktree),
        Ok(Standing::Submission { landed: true, .. } | Standing::PullRequest { landed: true, .. })
    )
}

/// Whether worktrunk removes a worktree once it lands: the user's
/// `[merge] remove`, true when unset, as `wt merge` reads it.
pub fn removes_on_landing(worktree: &Path) -> bool {
    let out = Command::new("wt")
        .arg("-C")
        .arg(worktree)
        .args(["config", "show", "--format", "json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return true;
    };
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()
        .and_then(|v| v["user"]["config"]["merge"]["remove"].as_bool())
        .unwrap_or(true)
}

/// Session-runs-1 clause 95, C.5 worktrunk: `sluice release` in the
/// landed session's worktree, before worktrunk removes it, so a
/// `post-remove` hook finds nothing of its runs left. What it says goes to
/// stderr, so a `--json` submit's stdout stays its one document.
pub fn release(worktree: &Path) -> Result<()> {
    let status = Command::new("sluice")
        .arg("release")
        .arg("--worktree")
        .arg(worktree)
        .stdin(Stdio::null())
        .stdout(std::io::stderr())
        .status()
        .context("running sluice release")?;
    if !status.success() {
        bail!("sluice release exited {status}");
    }
    Ok(())
}

/// `wt remove` of the landed session's worktree, in the foreground so it is
/// gone when `wt submit` exits; its branch goes with it once merged. Its
/// own hooks need approval as any `wt remove`'s do. What it says goes to
/// stderr, as [`release`]'s does.
pub fn remove(worktree: &Path) -> Result<()> {
    let status = Command::new("wt")
        .arg("-C")
        .arg(worktree)
        .args(["remove", "--foreground"])
        .stdin(Stdio::null())
        .stdout(std::io::stderr())
        .status()
        .context("running wt remove")?;
    if !status.success() {
        bail!("wt remove exited {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C.4: a waiting submit from one worktree removes it once it lands,
    /// unless told not to; one that does not wait never does, nor does
    /// asking for help or arguments sluice refuses. A `--no-wait` submit
    /// from one worktree reads what stands as a waiting one does.
    /// `--timeout` is `wt submit`'s own, kept for the wait and refused on
    /// a run that does not wait, or with a value sluice cannot read.
    #[test]
    fn what_a_submit_removes() {
        let parse = |a: &[&str]| Submit::parse(a.iter().map(OsString::from));
        // The branch and worktree a run removes once it lands.
        let removes = |s: &Submit| {
            s.single
                .as_ref()
                .filter(|w| w.waits && w.remove)
                .map(|w| (w.branch.clone(), w.worktree.clone()))
        };
        let at = |b: &str, w: &str| Some((b.to_string(), PathBuf::from(w)));
        let s = parse(&["main", "--json"]);
        assert_eq!(s.sluice, ["submit", "main", "--json"]);
        assert_eq!(removes(&s), at("main", "."));
        assert!(s.single.as_ref().is_some_and(|w| w.json && !w.orders));
        assert_eq!(s.refused, None);
        // Kept, and still a waiting run, which a run again follows with.
        let s = parse(&["main", "--no-remove"]);
        assert_eq!(removes(&s), None);
        assert!(s.single.as_ref().is_some_and(|w| w.waits && !w.remove));
        assert_eq!(s.sluice, ["submit", "main"]);
        for reads in [
            &["main", "--check"][..],
            &["main", "--status"],
            &["main", "--head", "feature/x"],
        ] {
            let s = parse(reads);
            assert_eq!(s.single, None, "{reads:?}");
            assert_eq!(s.refused, None, "{reads:?}");
        }
        // Without waiting, from one worktree: it reads what stands, and
        // removes nothing.
        let s = parse(&["main", "--no-wait", "--json", "--worktree", "../s"]);
        assert_eq!(removes(&s), None);
        assert_eq!(s.refused, None);
        assert_eq!(
            s.single,
            Some(Single {
                branch: "main".into(),
                worktree: "../s".into(),
                waits: false,
                remove: true,
                timeout: None,
                json: true,
                orders: false,
            })
        );
        assert_eq!(
            s.sluice,
            [
                "submit",
                "main",
                "--no-wait",
                "--json",
                "--worktree",
                "../s"
            ]
        );
        let s = parse(&["main", "--no-wait", "--after", "7"]);
        assert!(s.single.is_some_and(|w| !w.waits && w.orders && !w.json));
        let w = parse(&["main", "--worktree", "../s"]);
        assert_eq!(removes(&w), at("main", "../s"));
        assert_eq!(w.sluice, ["submit", "main", "--worktree", "../s"]);
        let w = parse(&["main", "--worktree=../t"]);
        assert_eq!(removes(&w), at("main", "../t"));
        let w = parse(&["--priority", "3", "main", "--after", "1,2"]);
        assert_eq!(removes(&w), at("main", "."));
        assert!(w.single.as_ref().is_some_and(|w| w.orders));
        let w = parse(&["main", "--priority", "0"]);
        assert!(w.single.as_ref().is_some_and(|w| w.orders));
        for several in [
            &["main", "--worktree", "../a", "--worktree", "../b"][..],
            &[
                "main",
                "--no-wait",
                "--worktree",
                "../a",
                "--worktree",
                "../b",
            ],
        ] {
            assert_eq!(parse(several).single, None, "{several:?}");
        }
        for asks in [
            &["--help"][..],
            &["-h"],
            &["main", "--help"],
            &["--version"],
            &[],
            &["main", "--no-such-flag"],
        ] {
            let s = parse(asks);
            assert_eq!(s.single, None, "{asks:?}");
            assert_eq!(s.sluice.len(), asks.len() + 1, "{asks:?}");
        }

        // `--timeout`, in either form, is kept for the wait and never
        // forwarded to sluice.
        for given in [
            &["main", "--timeout", "9m", "--json"][..],
            &["main", "--timeout=9m", "--json"],
        ] {
            let s = parse(given);
            assert_eq!(s.sluice, ["submit", "main", "--json"], "{given:?}");
            assert_eq!(s.refused, None, "{given:?}");
            let w = s.single.expect("a waiting run");
            assert_eq!(w.timeout, Some(OsString::from("9m")), "{given:?}");
            assert!(w.waits && w.remove && w.json, "{given:?}");
        }
        let s = parse(&["main", "--timeout", "1h30m", "--no-remove"]);
        assert_eq!(s.refused, None);
        assert!(s
            .single
            .is_some_and(|w| !w.remove && w.timeout == Some("1h30m".into())));
        // A place in the queue, with a time limit: refused only where a
        // submission stands, which the run reads.
        for given in [
            &["main", "--timeout", "9m", "--after", "3"][..],
            &["main", "--priority", "2", "--timeout", "9m"],
        ] {
            let s = parse(given);
            assert_eq!(s.refused, None, "{given:?}");
            assert!(
                s.single
                    .is_some_and(|w| w.orders && w.timeout == Some("9m".into())),
                "{given:?}"
            );
        }
        // Refused with nothing run where a run does not wait.
        for (given, why) in [
            (
                &["main", "--timeout", "9m", "--no-wait"][..],
                "this one gives --no-wait",
            ),
            (
                &["main", "--check", "--timeout", "9m"],
                "this one gives --check",
            ),
            (
                &["main", "--timeout=9m", "--status"],
                "this one gives --status",
            ),
            (
                &[
                    "main",
                    "--timeout",
                    "9m",
                    "--worktree",
                    "../a",
                    "--worktree",
                    "../b",
                ],
                "this one submits several worktrees",
            ),
            (
                &["main", "--timeout", "9m", "--head", "feature/x"],
                "this one submits a branch by name",
            ),
        ] {
            let s = parse(given);
            assert_eq!(
                s.refused,
                Some(format!("--timeout is for a run that waits, and {why}")),
                "{given:?}"
            );
        }
        // A value sluice cannot read, or none.
        let s = parse(&["main", "--timeout", "soon"]);
        assert!(
            s.refused
                .as_ref()
                .is_some_and(|r| r.starts_with("--timeout soon: ")),
            "{:?}",
            s.refused
        );
        assert!(parse(&["main", "--timeout=9x"]).refused.is_some());
        assert_eq!(
            parse(&["main", "--timeout"]).refused,
            Some("--timeout takes a duration, as 9m".into())
        );
    }

    /// Record 0065, 124: a run again follows a submission of the revision
    /// sluice says a submit would send, undecided or landed, or its pull
    /// request open and ready there; anything else is submitted, and an
    /// answer sluice did not give, or one that names no such revision, is
    /// no answer.
    #[test]
    fn what_stands_at_what_a_submit_sends() {
        const SENDS: &str = "0123456789abcdef0123456789abcdef01234567";
        const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";
        let read = |code: i32, doc: serde_json::Value| {
            standing(Some(code), doc.to_string().as_bytes(), b"")
        };
        let record = |state: &str, head: &str| {
            serde_json::json!({
                "id": 7, "branch": "main", "state": state, "head": head, "sends": SENDS,
            })
        };
        for state in ["queued", "waiting", "testing", "passed", "landing"] {
            assert_eq!(
                read(0, record(state, SENDS)),
                Ok(Standing::Submission {
                    id: 7,
                    landed: false
                }),
                "{state}"
            );
        }
        assert_eq!(
            read(0, record("landed", SENDS)),
            Ok(Standing::Submission {
                id: 7,
                landed: true
            })
        );
        for state in ["failed", "conflict", "withdrawn"] {
            assert_eq!(
                read(0, record(state, SENDS)),
                Ok(Standing::Nothing),
                "{state}"
            );
        }
        assert_eq!(read(0, record("queued", OTHER)), Ok(Standing::Nothing));
        assert_eq!(read(0, record("landed", OTHER)), Ok(Standing::Nothing));
        // 124: from a worktree on a branch sluice lands on, its copy
        // behind, the record is of the remote's revision, which is what a
        // submit sends; the worktree's own head is never read.
        let bolt = serde_json::json!({
            "id": 8, "branch": "main", "state": "queued", "head": OTHER, "sends": OTHER,
            "copy": { "branch": "bolt/x", "copy": SENDS, "at": OTHER, "how": "behind" },
        });
        assert_eq!(
            read(0, bolt),
            Ok(Standing::Submission {
                id: 8,
                landed: false
            })
        );

        let pull = |state: &str, head: Option<&str>, draft: bool| {
            serde_json::json!({
                "pull_request": 12, "branch": "main", "state": state,
                "head": head, "draft": draft, "sends": SENDS,
            })
        };
        assert_eq!(
            read(0, pull("waiting: needs 1 review", Some(SENDS), false)),
            Ok(Standing::PullRequest {
                number: 12,
                landed: false
            })
        );
        assert_eq!(
            read(0, pull("waiting: a draft", Some(SENDS), true)),
            Ok(Standing::Nothing)
        );
        assert_eq!(
            read(0, pull("waiting", Some(OTHER), false)),
            Ok(Standing::Nothing)
        );
        assert_eq!(read(0, pull("waiting", None, false)), Ok(Standing::Nothing));
        assert_eq!(
            read(
                1,
                serde_json::json!({ "pull_request": 12, "branch": "main", "state": "closed" })
            ),
            Ok(Standing::Nothing)
        );
        // Record 0096, 137: sent back on another host, failed or in
        // conflict, draft or not, it stands nowhere; landed there, it
        // stands at the revision it landed.
        for state in ["failed", "conflict"] {
            for draft in [true, false] {
                assert_eq!(
                    read(0, pull(state, Some(SENDS), draft)),
                    Ok(Standing::Nothing),
                    "{state}, draft {draft}"
                );
            }
        }
        assert_eq!(
            read(0, pull("landed", Some(SENDS), false)),
            Ok(Standing::PullRequest {
                number: 12,
                landed: true
            })
        );
        assert_eq!(
            read(0, pull("landed", Some(OTHER), false)),
            Ok(Standing::Nothing)
        );
        // A document that names no such revision is no answer, sent back
        // or not.
        let unsent = serde_json::json!({
            "pull_request": 12, "branch": "main", "state": "failed",
            "head": SENDS, "draft": true,
        });
        assert!(read(0, unsent).is_err());

        // Never submitted from this worktree.
        let none = serde_json::json!({ "error": {
            "kind": "not_found", "message": "nothing submitted from . to main",
            "next": ["sluice submit main"],
        }});
        assert_eq!(read(1, none), Ok(Standing::Nothing));

        // No answer: sluice's words, never Nothing.
        let internal = serde_json::json!({ "error": {
            "kind": "internal", "message": "reading main's records: refused", "next": [],
        }});
        assert_eq!(
            read(3, internal.clone()),
            Err("sluice submit --status exited 3: ".to_string() + &internal.to_string())
        );
        assert_eq!(
            read(1, internal),
            Err("reading main's records: refused".to_string())
        );
        // A record or a pull request that names no `sends`.
        let unsent =
            serde_json::json!({ "id": 7, "branch": "main", "state": "queued", "head": SENDS });
        let e = read(0, unsent.clone()).unwrap_err();
        assert_eq!(e, format!("sluice submit --status exited 0: {unsent}"));
        let unsent = serde_json::json!({
            "pull_request": 12, "branch": "main", "state": "waiting", "head": SENDS, "draft": false,
        });
        assert!(read(0, unsent).is_err());
        let e = standing(Some(0), b"not json", b"").unwrap_err();
        assert!(e.contains("not json"), "{e}");
        let e = standing(Some(0), b"{}", b"").unwrap_err();
        assert!(e.contains("{}"), "{e}");
        let e = standing(Some(64), b"", b"error: unexpected argument").unwrap_err();
        assert_eq!(
            e,
            "sluice submit --status exited 64: error: unexpected argument"
        );
        assert!(standing(None, b"", b"").is_err());
    }

    /// `--subject` is sluice's, handed on as given, its words one
    /// argument: a submit that does not wait removes nothing, and one that
    /// waits removes its worktree as without it.
    #[test]
    fn a_subject_is_handed_on_whole() {
        let parse = |a: &[&str]| Submit::parse(a.iter().map(OsString::from));
        let s = parse(&["main", "--no-wait", "--subject", "Two words"]);
        assert_eq!(
            s.sluice,
            ["submit", "main", "--no-wait", "--subject", "Two words"]
        );
        assert_eq!(s.refused, None);
        assert!(s.single.is_some_and(|w| !w.waits));
        let s = parse(&["main", "--subject", "Two words"]);
        assert_eq!(
            s.single,
            Some(Single {
                branch: "main".into(),
                worktree: ".".into(),
                waits: true,
                remove: true,
                timeout: None,
                json: false,
                orders: false,
            })
        );
    }

    /// git's own bytes for a staged deletion, a modification staged and
    /// not, a rename, an addition, and untracked files with a space and
    /// in a new directory (section 7).
    #[test]
    fn every_path_held_beyond_the_commits() {
        let raw = b"D  .config/wt.toml\0MM .gitattributes\0 M a.txt\0R  b-renamed.txt\0b.txt\0A  staged.txt\0?? .config/wt.toml\0?? sp ace.txt\0?? sub/new.txt\0";
        assert_eq!(
            dirty(raw),
            vec![
                "D  .config/wt.toml",
                "MM .gitattributes",
                " M a.txt",
                "R  b-renamed.txt <- b.txt",
                "A  staged.txt",
                "?? .config/wt.toml",
                "?? sp ace.txt",
                "?? sub/new.txt",
            ]
        );
        assert!(dirty(b"").is_empty());
    }

    #[test]
    fn wt_submit_is_sluice_submit() {
        let args = forward(["bolt/x".into(), "--no-wait".into()]);
        assert_eq!(args, vec!["submit", "bolt/x", "--no-wait"]);
    }
}
