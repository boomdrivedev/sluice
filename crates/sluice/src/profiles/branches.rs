//! The branch seam as an instance binds it: each branch to the profile
//! its policy chooses, `pushed refs` for a branch that names a `remote`
//! and `local refs` for any other (C.4, C.7 `remote`). Calls that name no
//! branch go to local refs, since every object is on this host.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{anyhow, bail, Context, Result};
use sluice_core::policy::{Policy, POLICY_PATH};
use sluice_seams::{Branch, Formed, HostCopy, LandingCommit, Moved, Rev};

use super::local_refs::LocalRefs;
use super::pushed_refs::PushedRefs;
use crate::identity::Identity;
use crate::retry;
use crate::{Pusher, Pushers};

pub struct Branches {
    local: LocalRefs,
    dir: PathBuf,
    identity: Option<(Rc<dyn Identity>, PathBuf)>,
    /// The retry policy every call to a remote runs under (134).
    retry: Rc<retry::Policy>,
    pushed: RefCell<BTreeMap<String, Rc<PushedRefs>>>,
    /// 129: each policy the routing read, by the revision it was read at:
    /// as far as guarding needs, or the words it was refused with; and the
    /// revisions this pass used, the rest dropped at the next.
    guarded: RefCell<BTreeMap<Rev, std::result::Result<Option<Policy>, String>>>,
    used: RefCell<std::collections::BTreeSet<Rev>>,
}

impl Branches {
    /// The repository holding `path`, a worktree or a git directory, its
    /// remotes reached under a command's retry policy.
    pub fn open(path: &Path) -> Result<Self> {
        Branches::open_as(path, None, Rc::new(retry::Policy::command()))
    }

    /// As [`Branches::open`], each remote reached as `identity` where it
    /// has one on the remote's forge, each call under `retry`; the
    /// `PathBuf` is the `sluice` executable git runs as its credential
    /// helper.
    pub fn open_as(
        path: &Path,
        identity: Option<(Rc<dyn Identity>, PathBuf)>,
        retry: Rc<retry::Policy>,
    ) -> Result<Self> {
        let local = LocalRefs::open(path)?;
        Ok(Branches {
            dir: local.git_dir().to_path_buf(),
            local,
            identity,
            retry,
            pushed: RefCell::default(),
            guarded: RefCell::default(),
            used: RefCell::default(),
        })
    }

    /// 129: start a pass, a tick or a wait's read: the policies the last
    /// did not use are forgotten.
    pub fn next_pass(&self) {
        let used = std::mem::take(&mut *self.used.borrow_mut());
        self.guarded
            .borrow_mut()
            .retain(|rev, _| used.contains(rev));
    }

    /// The repository's git directory.
    pub fn git_dir(&self) -> &Path {
        &self.dir
    }

    /// The repository's common git directory.
    pub fn common_dir(&self) -> Result<PathBuf> {
        self.local.common_dir()
    }

    /// The repository's remotes, read with no process (129).
    pub fn remotes(&self) -> Result<Vec<String>> {
        self.local.remotes()
    }

    /// The repository's default branch, read with no process (129).
    pub fn default_branch(&self) -> Result<Option<String>> {
        self.local.default_branch()
    }

    /// The repository, as gix reads its refs and objects.
    pub fn local(&self) -> &LocalRefs {
        &self.local
    }

    /// The remote `branch` is on: the one the policy at any revision of
    /// it this host holds names, its own ref or a remote's tracking ref as
    /// last fetched, with no fetch made to decide it (124; rule core.27).
    /// Each revision's policy is read only as far as the deciding needs,
    /// the remote it names for `branch` and whether it names signers, so a
    /// revision refused elsewhere in its policy decides as any other does
    /// (record 0069). A revision that lags, holds no policy or names no
    /// remote decides nothing against one that names a remote, and two
    /// naming different remotes are refused. The gate, and the rest of the
    /// policy, are read whole from the revision the profile reads (56;
    /// rule core.15).
    pub fn remote(&self, branch: &str) -> Result<Option<String>> {
        Ok(self.routing(branch, branch)?.remote)
    }

    /// The remote `branch` would be made on from `from` (branches-1 104):
    /// the one the policy at any revision of `from` this host holds names
    /// for `branch`, each read as [`Branches::remote`] reads it, only as far
    /// as the deciding needs, with no fetch (124; record 0069; rule
    /// core.27).
    pub fn remote_if_made_from(&self, branch: &str, from: &str) -> Result<Option<String>> {
        Ok(self.routing(branch, from)?.remote)
    }

    /// The policy `branch` is routed by, and the revision it was read at:
    /// the first revision this host holds whose policy names the remote
    /// [`Branches::remote`] reads, or, when none names one, the first
    /// holding a policy, with no fetch (124; rule core.27). The policy is
    /// read only as far as the deciding needs, the remote each pattern
    /// names and whether signers are named, every other table and field
    /// left at its defaults (record 0069).
    pub fn policy_of(&self, branch: &str) -> Result<Option<(Rev, Policy)>> {
        Ok(self.routing(branch, branch)?.at)
    }

    /// A file of a revision's tree.
    pub fn file_at(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        self.local.read_file(rev, path)
    }

    /// Whether this host has fetched any branch's state from `remote`,
    /// read with no process (129).
    pub fn fetched_state(&self, remote: &str) -> Result<bool> {
        Ok(!self
            .local
            .refs_under(&format!("refs/sluice-fetched/{remote}/state/"))?
            .is_empty())
    }

    /// The pushed refs profile for `remote`, as this binding reaches it.
    pub fn pushed_refs(&self, remote: &str) -> Result<Rc<PushedRefs>> {
        self.pushed(remote, remote)
    }

    /// The revisions of `branch` this host holds, each with its ref as the
    /// user knows it: its own ref first, then each remote's tracking ref
    /// of it as last fetched, remotes in the order git lists them (124).
    /// Nothing is fetched.
    fn revisions(&self, branch: &str) -> Result<Vec<(String, Rev)>> {
        let mut out = Vec::new();
        if let Some(rev) = self.local.read(branch)? {
            out.push((branch.to_string(), rev));
        }
        for r in self.local.remotes()? {
            if let Some(rev) = self.local.read_ref(&format!("refs/remotes/{r}/{branch}"))? {
                out.push((format!("{r}/{branch}"), rev));
            }
        }
        Ok(out)
    }

    /// Where `branch` is, read off the revisions of `from` this host
    /// holds, as [`routing`] decides it from their policies.
    fn routing(&self, branch: &str, from: &str) -> Result<Routing> {
        routing(branch, from, self.revisions(from)?, |name, rev| {
            self.policy_at(name, rev)
        })
    }

    /// The remote the policy at `rev` names for `branch`, read as the
    /// routing reads it (124; record 0069).
    fn named(&self, branch: &str, rev: &Rev) -> Result<Option<String>> {
        let Some(policy) = self.policy_at(branch, rev)? else {
            return Ok(None);
        };
        policy.remote_of(branch)
    }

    /// The policy at `rev`, read from the ref the user knows as `name`,
    /// only as far as deciding where a branch is needs: the remote each
    /// `[branches]` pattern names and whether `[signers]` names a signer,
    /// [`Policy::guarding`], every other table and field passed over,
    /// whatever this binding is for (124; record 0069; rule core.27). Only
    /// a `remote` or a `[signers]` it can't read refuses, naming `name` and
    /// `rev`.
    ///
    /// 129: read from git once per revision while it is kept, a refusal in
    /// the same words; a read that failed is made again.
    fn policy_at(&self, name: &str, rev: &Rev) -> Result<Option<Policy>> {
        self.used.borrow_mut().insert(rev.clone());
        let kept = self.guarded.borrow().get(rev).cloned();
        let decided = match kept {
            Some(d) => d,
            None => {
                let decided = guarded(self.local.read_file(rev, POLICY_PATH)?.as_deref());
                self.guarded
                    .borrow_mut()
                    .insert(rev.clone(), decided.clone());
                decided
            }
        };
        read_as(name, rev, decided)
    }

    /// The profile for `branch`: its remote's, or `None` for local refs.
    fn route(&self, branch: &str) -> Result<Option<Rc<PushedRefs>>> {
        let Some(remote) = self.remote(branch)? else {
            return Ok(None);
        };
        Ok(Some(self.pushed(&remote, branch)?))
    }

    /// The pushed refs profile for `remote`, opened once.
    fn pushed(&self, remote: &str, branch: &str) -> Result<Rc<PushedRefs>> {
        if let Some(p) = self.pushed.borrow().get(remote) {
            return Ok(p.clone());
        }
        let p = Rc::new(
            PushedRefs::open_as(&self.dir, remote, self.identity.clone(), self.retry.clone())
                .with_context(|| format!("{branch}'s remote"))?,
        );
        self.pushed.borrow_mut().insert(remote.into(), p.clone());
        Ok(p)
    }
}

/// What the revisions of a branch the host holds say of where it is: the
/// remote they name, if any, and the revision and policy it is routed by.
struct Routing {
    remote: Option<String>,
    at: Option<(Rev, Policy)>,
}

/// The policy a file holds, `None` for none, read only as far as deciding
/// where a branch is needs ([`Policy::guarding`]; record 0069); or the
/// words it was refused with.
fn guarded(file: Option<&[u8]>) -> std::result::Result<Option<Policy>, String> {
    match file {
        None => Ok(None),
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(anyhow::Error::from)
            .and_then(Policy::guarding)
            .map(Some)
            .map_err(|e| format!("{e:#}")),
    }
}

/// `decided`, the policy at `rev` of the ref the user knows as `name`,
/// a refusal naming both.
fn read_as(
    name: &str,
    rev: &Rev,
    decided: std::result::Result<Option<Policy>, String>,
) -> Result<Option<Policy>> {
    decided.map_err(|words| anyhow!(words).context(format!("{name}'s policy at {rev}")))
}

/// Where `branch` is, read off `revisions`, the revisions of `from` this
/// host holds, each with its ref as the user knows it, its own first (124;
/// rule core.27): the remote any of their policies names for `branch`,
/// with the first revision naming it and its policy, or, when none names
/// one, the first revision holding a policy. `policy_at` reads each
/// revision's policy only as far as the deciding needs, the remote named
/// for `branch` and whether signers are named, so a revision this sluice
/// refuses elsewhere in its policy votes as every other does (record
/// 0069). A revision that holds no policy or names no remote decides
/// nothing; two naming different remotes are refused, naming both, since
/// nothing the host holds says which is right (record 0060).
fn routing(
    branch: &str,
    from: &str,
    revisions: Vec<(String, Rev)>,
    mut policy_at: impl FnMut(&str, &Rev) -> Result<Option<Policy>>,
) -> Result<Routing> {
    let mut named: Option<(String, String, Rev, Policy)> = None;
    let mut first: Option<(Rev, Policy)> = None;
    let mut read: Vec<Rev> = Vec::new();
    for (name, rev) in revisions {
        if read.contains(&rev) {
            continue;
        }
        read.push(rev.clone());
        let Some(policy) = policy_at(&name, &rev)? else {
            continue;
        };
        // core.27 (design D22): the remote whatever the table refuses.
        let remote = policy.remote_of(branch)?;
        match (remote, &named) {
            (Some(remote), None) => named = Some((remote, name, rev, policy)),
            (Some(remote), Some((was, by, at, _))) if remote != *was => {
                let (what, which) = match branch == from {
                    true => (format!("{branch}'s policy names"), "is on"),
                    false => (
                        format!("{from}'s policy names, for {branch},"),
                        "would be made on",
                    ),
                };
                bail!(
                    "{what} two remotes: {was} on {by} ({}), {remote} on {name} ({}); \
                     sluice cannot tell which remote {branch} {which}",
                    at.short(),
                    rev.short()
                );
            }
            (Some(_), Some(_)) => {}
            (None, _) if first.is_none() => first = Some((rev, policy)),
            (None, _) => {}
        }
    }
    Ok(match named {
        Some((remote, _, rev, policy)) => Routing {
            remote: Some(remote),
            at: Some((rev, policy)),
        },
        None => Routing {
            remote: None,
            at: first,
        },
    })
}

impl Pushers for Branches {
    fn pusher(&self, branch: &str) -> Result<Option<Pusher>> {
        let Some(p) = self.route(branch)? else {
            return Ok(None);
        };
        Ok(Some(Pusher {
            remote: p.remote().into(),
            identity: p.identity()?,
        }))
    }
}

impl Branch for Branches {
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        match self.route(branch)? {
            Some(p) => p.read(branch),
            None => self.local.read(branch),
        }
    }

    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        match self.route(branch)? {
            Some(p) => p.held(branch),
            None => self.local.held(branch),
        }
    }

    fn holds(&self, rev: &Rev) -> Result<bool> {
        self.local.holds(rev)
    }

    fn copy(&self, branch: &str) -> Result<HostCopy> {
        match self.route(branch)? {
            Some(p) => p.copy(branch),
            None => self.local.copy(branch),
        }
    }

    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        match self.route(branch)? {
            Some(p) => p.level(branch, expected, to),
            None => self.local.level(branch, expected, to),
        }
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        self.local.read_file(rev, path)
    }

    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        self.local.form(onto, head)
    }

    /// A landing is written by the profile its branch is routed to, so one
    /// pushed to a remote names whoever pushes it (record 0019 as amended
    /// 2026-11-18; rule core.43).
    fn commit(&self, branch: &str, commit: &LandingCommit) -> Result<Rev> {
        match self.route(branch)? {
            Some(p) => p.commit(branch, commit),
            None => self.local.commit(branch, commit),
        }
    }

    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool> {
        self.local.contains(tip, rev)
    }

    fn merge_base(&self, a: &Rev, b: &Rev) -> Result<Option<Rev>> {
        self.local.merge_base(a, b)
    }

    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        self.local.landed_by(tip, rev)
    }

    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        match self.route(branch)? {
            Some(p) => p.move_to(branch, expected, to),
            None => self.local.move_to(branch, expected, to),
        }
    }

    /// A branch not yet made has no revision to read its remote from; the
    /// policy at the revision it is made at names it.
    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        match self.named(branch, at)? {
            Some(remote) => self.pushed(&remote, branch)?.make(branch, at),
            None => self.local.make(branch, at),
        }
    }

    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        match self.route(branch)? {
            Some(p) => p.remove(branch, expected),
            None => self.local.remove(branch, expected),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `[[checkout.cache]]` table, which sluice no longer keeps and whose
    /// policy it refuses whole (ADR 0049).
    const CACHE: &str = "[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n";

    /// `branch` routed off the revisions of `from` in `held`, each its ref
    /// as the user knows it, its revision, and the policy file it holds.
    fn route(branch: &str, from: &str, held: &[(&str, &str, Option<&str>)]) -> Result<Routing> {
        let revisions = held
            .iter()
            .map(|(name, rev, _)| (name.to_string(), Rev::new(*rev)))
            .collect();
        routing(branch, from, revisions, |name, rev| {
            let file = held
                .iter()
                .find(|(_, r, _)| *r == rev.as_str())
                .and_then(|(_, _, f)| *f);
            read_as(name, rev, guarded(file.map(str::as_bytes)))
        })
    }

    const ORIGIN: &str =
        "[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\nremote = \"origin\"\n";

    /// 124 (rule core.27): a host's own `main` from before the policy file
    /// decides nothing, and origin's copy as last fetched routes `main` to
    /// origin, by origin's revision; a branch made from it is made there.
    /// Nothing is fetched to decide (`the_router_reads_a_copy_from_before_the_policy_as_origins`,
    /// `made_from_and_policy_of_read_a_copy_from_before_the_policy_as_origins`,
    /// `the_router_decides_with_the_remote_out_of_reach`).
    #[test]
    fn a_copy_from_before_the_policy_routes_by_the_remote_copy() {
        let held = [
            ("main", "aaaaaaa0", None),
            ("origin/main", "bbbbbbb0", Some(ORIGIN)),
        ];
        let r = route("main", "main", &held).unwrap();
        assert_eq!(r.remote.as_deref(), Some("origin"));
        assert_eq!(r.at.unwrap().0, Rev::new("bbbbbbb0"));
        let made = route("bolt/x", "main", &held).unwrap();
        assert_eq!(made.remote.as_deref(), Some("origin"));
    }

    /// 124: a host's `main` whose policy names no remote decides nothing
    /// against origin's later copy, which names origin; with no copy naming
    /// one, `main` stays local, routed by the host's own revision
    /// (`the_router_follows_a_later_copy_naming_a_remote`,
    /// `the_router_keeps_a_branch_no_copy_routes_local`).
    #[test]
    fn a_copy_naming_a_remote_routes_and_none_keeps_the_branch_local() {
        let none = "[branches.main]\n";
        let r = route(
            "main",
            "main",
            &[
                ("main", "aaaaaaa0", Some(none)),
                ("origin/main", "bbbbbbb0", Some(ORIGIN)),
            ],
        )
        .unwrap();
        assert_eq!(r.remote.as_deref(), Some("origin"));
        assert_eq!(r.at.unwrap().0, Rev::new("bbbbbbb0"));
        let r = route(
            "main",
            "main",
            &[
                ("main", "aaaaaaa0", Some(none)),
                ("origin/main", "bbbbbbb0", Some(none)),
            ],
        )
        .unwrap();
        assert_eq!(r.remote, None);
        assert_eq!(r.at.unwrap().0, Rev::new("aaaaaaa0"));
    }

    /// 124 (record 0060, 0069): two revisions naming different remotes are
    /// refused, naming each remote and its ref, whatever else one of them
    /// is refused for (`the_router_refuses_two_remotes_named`,
    /// `two_remotes_are_still_refused_when_one_copy_is_refused_elsewhere`).
    #[test]
    fn two_remotes_named_are_refused_naming_each() {
        let upstream = "[branches.main]\nremote = \"upstream\"\n";
        for own in [
            "[branches.main]\nremote = \"origin\"\n".to_string(),
            format!("[branches.main]\nremote = \"origin\"\n{CACHE}"),
        ] {
            let err = route(
                "main",
                "main",
                &[
                    ("main", "aaaaaaa0", Some(&own)),
                    ("upstream/main", "bbbbbbb0", Some(upstream)),
                ],
            )
            .err()
            .map(|e| format!("{e:#}"))
            .expect("refused");
            assert!(err.contains("origin on main (aaaaaaa)"), "{err}");
            assert!(err.contains("upstream on upstream/main (bbbbbbb)"), "{err}");
            assert!(!err.contains("cache"), "{err}");
        }
    }

    /// 124, record 0069: a revision whose policy sluice refuses elsewhere,
    /// for a `[[checkout.cache]]` table, still names its branch's remote,
    /// beside a clean copy and as the one revision naming it; and the policy
    /// it routes by keeps what the deciding reads, its signers among it
    /// (`a_copy_refused_elsewhere_still_says_where_the_branch_is`,
    /// `the_one_revision_naming_the_remote_is_refused_elsewhere`,
    /// `made_from_and_policy_of_read_a_refused_copy_as_remote_does`).
    #[test]
    fn a_revision_refused_elsewhere_still_routes() {
        let refused = format!("{ORIGIN}{CACHE}");
        assert!(Policy::parse(&refused).is_err());
        for held in [
            vec![
                ("bolt/x", "aaaaaaa0", Some(refused.as_str())),
                ("origin/bolt/x", "bbbbbbb0", Some(ORIGIN)),
            ],
            vec![("bolt/x", "aaaaaaa0", Some(refused.as_str()))],
        ] {
            let r = route("bolt/x", "bolt/x", &held).unwrap();
            assert_eq!(r.remote.as_deref(), Some("origin"));
        }
        let signed = format!("{ORIGIN}[signers.box]\nssh = \"keys/box.pub\"\n{CACHE}");
        let held = [("main", "aaaaaaa0", Some(signed.as_str()))];
        let r = route("main", "main", &held).unwrap();
        assert_eq!(r.remote.as_deref(), Some("origin"));
        let (rev, policy) = r.at.unwrap();
        assert_eq!(rev, Rev::new("aaaaaaa0"));
        assert_eq!(policy.signers.keys().collect::<Vec<_>>(), ["box"]);
        let made = route("bolt/x", "main", &held).unwrap();
        assert_eq!(made.remote.as_deref(), Some("origin"));
    }

    /// 124, record 0069: a `remote` that is not a remote's name refuses the
    /// routing, naming the copy, its revision and what `remote` must be
    /// (`a_remote_that_cant_be_read_refuses_naming_the_copy`).
    #[test]
    fn a_remote_that_cannot_be_read_refuses_naming_the_copy() {
        let err = route(
            "bolt/x",
            "bolt/x",
            &[(
                "bolt/x",
                "aaaaaaa0",
                Some("[branches.main]\n[branches.\"bolt/*\"]\nremote = 1\n"),
            )],
        )
        .err()
        .map(|e| format!("{e:#}"))
        .expect("refused");
        assert!(err.contains("bolt/x's policy at aaaaaaa"), "{err}");
        assert!(
            err.contains("[branches.\"bolt/*\"] remote must be the name of a git remote"),
            "{err}"
        );
    }
}
