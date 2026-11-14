//! The records seam as an instance binds it (C.6; ADR 0037): a branch
//! whose policy names a `remote` and signers to `remote refs` on that
//! remote, every other to `local files`. The results, which name no
//! branch, go to the remote once a branch has been routed there; a
//! branch's next number comes from the count of the records that hold
//! it, so a remote's are numbered once across every host and the host's
//! own once on the host (record 0114).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use sluice_core::policy::{Policy, POLICY_PATH};
use sluice_core::records::{parse_lines, to_line, ResultLine, SetAside};
use sluice_seams::{
    read_refused, Branch, Changes, Guard, ReadRefused, ResultAsk, Rev, Setting, Snapshot, State,
    TaskLog, TaskProgress, Written,
};

use super::branches::Branches;
use super::local_files::LocalFiles;
use super::remote_refs::{RemoteRefs, SignerKey, Signing};
use crate::instance;
use crate::signing::ssh;

/// What this host noted when a branch's own records moved to its remote:
/// the submissions it left behind, each its document.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Moved {
    #[serde(with = "docs")]
    left: Vec<Vec<u8>>,
}

/// The documents as JSON text.
mod docs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(docs: &[Vec<u8>], s: S) -> Result<S::Ok, S::Error> {
        let text: Vec<String> = docs
            .iter()
            .map(|d| String::from_utf8_lossy(d).into_owned())
            .collect();
        text.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<u8>>, D::Error> {
        Ok(Vec::<String>::deserialize(d)?
            .into_iter()
            .map(String::into_bytes)
            .collect())
    }
}

pub struct Records {
    local: LocalFiles,
    /// The locks of the branches routed to a remote: one host's steps,
    /// one at a time (core.14). Each remote's profile shares them, so a
    /// round keeps its tasks under the lock it took.
    locks: Rc<LocalFiles>,
    branches: Branches,
    common: PathBuf,
    signing: Option<instance::Signing>,
    remotes: RefCell<BTreeMap<String, Rc<RemoteRefs>>>,
    routes: RefCell<BTreeMap<String, Option<String>>>,
    /// The branches whose state is on each remote, as listed once before
    /// a tick asked it.
    listed: RefCell<Option<BTreeMap<String, Vec<String>>>>,
    /// 129: the remotes a tick has asked, whose branches are listed from
    /// what the ask fetched.
    asked: RefCell<std::collections::BTreeSet<String>>,
    /// 129 (design D16): whether every remote is read as the host
    /// process's last ask fetched it, and none is asked ([`State::hold`]).
    held: Cell<bool>,
    /// 124: whether it is bound for branch protection, which verifies and
    /// signs no run.
    guarding: bool,
    /// 78: each branch's signers and their keys, as its own policy names
    /// them, with the revision they were read at, or none where no
    /// revision was there to read; read again only once the branch's
    /// revision moves (56; rule core.31).
    lists: RefCell<BTreeMap<String, (Option<Rev>, Signers)>>,
    /// How many times a branch's policy was read whole for its signers.
    lists_read: Cell<u64>,
    /// The public half of the key this host signs with, once read.
    own_key: RefCell<Option<ssh_key::PublicKey>>,
    /// Where the results log is read from (design D17).
    results_from: ResultsFrom,
}

/// Where a binding reads the results log from (69, 129; design D17).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResultsFrom {
    /// The log and each signer's runs, read by the binding itself: a
    /// binding that no host process serves, a test's or the hook's.
    #[default]
    Here,
    /// The host process's index of each log, in the host process itself:
    /// a repository's task's binding and a connection's.
    Indexed,
    /// The host process, asked with `results` and started where none
    /// answers: a command's. Where it cannot be asked, the command reads
    /// them itself, and says so.
    Asked,
}

/// A branch's signers and the keys its runs are verified with (78).
type Signers = Rc<BTreeMap<String, SignerKey>>;

impl Records {
    pub fn new(common: &Path, branches: Branches, signing: Option<instance::Signing>) -> Self {
        Records {
            local: LocalFiles::new(common),
            locks: Rc::new(LocalFiles::at(common.join("sluice-remote").join("locks"))),
            branches,
            common: common.to_path_buf(),
            signing,
            remotes: RefCell::default(),
            routes: RefCell::default(),
            listed: RefCell::default(),
            asked: RefCell::default(),
            held: Cell::new(false),
            guarding: false,
            lists: RefCell::default(),
            lists_read: Cell::new(0),
            own_key: RefCell::default(),
            results_from: ResultsFrom::Here,
        }
    }

    /// The same binding, reading the results log from `from` (design D17).
    pub fn reading_results(self, from: ResultsFrom) -> Self {
        Records {
            results_from: from,
            ..self
        }
    }

    /// The results `ask` names, as `branch`'s gate reads them, read by this
    /// binding itself from the log and each signer's runs (78; rule
    /// core.22).
    fn results_here(&self, branch: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        let mut bytes = self.local.results_for(branch, ask)?;
        let routed = self.route(branch)?;
        let remotes: Vec<Rc<RemoteRefs>> = self.remotes.borrow().values().cloned().collect();
        if remotes.is_empty() {
            return Ok(bytes);
        }
        let signers = self.signers_of(branch, routed.as_ref().map(|r| r.remote()))?;
        for r in remotes {
            bytes.extend(ask.answer(&r.results_with(&signers)?));
        }
        Ok(bytes)
    }

    /// The same, from the host process's index of each log: the host's
    /// own results, then each remote's replays and the runs each signer
    /// `branch`'s policy names signed there, as the host last fetched
    /// them, a statement verified once for each key (design D17). Each
    /// answer that verified a statement says how many in the host
    /// process's log.
    fn results_indexed(&self, branch: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        use crate::host_process::results::{self, SignerRef};
        let own = results::of(&self.local.results_path());
        let mut bytes = own.answer(ask, &[])?;
        let routed = self.route(branch)?;
        let remotes: Vec<Rc<RemoteRefs>> = self.remotes.borrow().values().cloned().collect();
        if remotes.is_empty() {
            return Ok(bytes);
        }
        let signers = self.signers_of(branch, routed.as_ref().map(|r| r.remote()))?;
        for r in remotes {
            let index = results::of(&r.own_results_path());
            let refs: Vec<_> = signers
                .iter()
                .map(|(name, key)| r.signer_ref(name, key))
                .collect();
            let dyns: Vec<&dyn SignerRef> = refs.iter().map(|f| f as &dyn SignerRef).collect();
            let before = index.verifications();
            bytes.extend(index.answer(ask, &dyns)?);
            let verified = index.verifications() - before;
            if verified > 0 {
                crate::host_process::log(&format!(
                    "sluice host: {}: verified {verified} signed run record(s) on {}",
                    self.common.display(),
                    r.remote()
                ));
            }
        }
        Ok(bytes)
    }

    /// How many times this binding read a branch's policy whole for the
    /// signers its runs are verified with: once a revision the branch
    /// moves to, never again while it stands (rule core.31).
    pub fn lists_read(&self) -> u64 {
        self.lists_read.get()
    }

    /// The same binding, for branch protection: a remote's profile is
    /// given no signer's key and no signing, and no fetch is made to read
    /// them, since the hook verifies and signs no run (124). Where each
    /// branch is is read as every command reads it (record 0069).
    pub fn guarding(self) -> Self {
        Records {
            guarding: true,
            ..self
        }
    }

    /// 129: start a pass: the policies its routing read and the last pass
    /// did not use are forgotten.
    pub fn next_pass(&self) {
        self.branches.next_pass();
    }

    /// The host's own records, for what only this host keeps.
    pub fn local(&self) -> &LocalFiles {
        &self.local
    }

    /// The `remote refs` profile `branch` is routed to, or `None` for
    /// local files: its remote's when the policy the routing reads names
    /// signers and a remote for it (C.6; 27), each revision's policy read
    /// only as far as that needs (124; record 0069). The signers its runs
    /// are verified by are not the routing's: they are the branch's own,
    /// read from its revision on the remote, never from the host's own
    /// copy nor from another branch's policy ([`Records::signers_of`]; 56,
    /// 78, 124; rule core.22).
    pub fn route(&self, branch: &str) -> Result<Option<Rc<RemoteRefs>>> {
        if let Some(known) = self.routes.borrow().get(branch) {
            return Ok(known.as_ref().map(|r| self.remotes.borrow()[r].clone()));
        }
        let routed = match self.branches.policy_of(branch)? {
            Some((_, policy)) if !policy.signers.is_empty() => match policy.remote_of(branch)? {
                Some(remote) => Some(self.open(&remote)?),
                None => None,
            },
            _ => None,
        };
        self.routes.borrow_mut().insert(
            branch.into(),
            routed.as_ref().map(|r| r.remote().to_string()),
        );
        Ok(routed)
    }

    /// The remotes that may hold a branch's state: those the default
    /// branch's policy, read here with no fetch, names for a branch while
    /// it names signers, and those this host has fetched a state from
    /// before. A repository with no signers asks no remote (C.6). A
    /// default branch whose policy cannot be read names none: its refusal
    /// is its own, said wherever that branch is read, and the remotes this
    /// host has read a state from are listed all the same (26).
    fn signed_remotes(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if let Some((default, _)) = super::default_branch(self.branches.local())? {
            if let Ok(Some((_, policy))) = self.branches.policy_of(&default) {
                if !policy.signers.is_empty() {
                    for (_, rules) in &policy.branches {
                        if let Some(r) = &rules.remote {
                            if !out.contains(r) {
                                out.push(r.clone());
                            }
                        }
                    }
                }
            }
        }
        for remote in self.branches.remotes()? {
            if !out.contains(&remote) && self.branches.fetched_state(&remote)? {
                out.push(remote);
            }
        }
        Ok(out)
    }

    /// `remote`'s profile, opened once for every branch routed there. It
    /// reads no signer: each branch's runs on it are verified with the
    /// signers that branch's own policy names ([`Records::signers_of`]; 27,
    /// 78; rule core.22). Bound for guarding, it is given no signing.
    fn open(&self, remote: &str) -> Result<Rc<RemoteRefs>> {
        if let Some(r) = self.remotes.borrow().get(remote) {
            return Ok(r.clone());
        }
        let signing = match self.signing.as_ref().filter(|_| !self.guarding) {
            Some(s) => Some(Signing {
                signer: s.signer.clone(),
                key: ssh::private_key(&s.key)?,
            }),
            None => None,
        };
        let r = Rc::new(
            RemoteRefs::new(&self.common, self.branches.pushed_refs(remote)?, signing)
                .sharing_locks(self.locks.clone()),
        );
        if self.held.get() {
            r.hold();
        }
        self.remotes.borrow_mut().insert(remote.into(), r.clone());
        Ok(r)
    }

    /// The signers `branch`'s runs are verified with, and their keys: those
    /// its own policy names, read whole, at its revision (78; 27, 56;
    /// rule core.22), never another branch's, whichever was read first.
    ///
    /// For a branch on its remote, the revision is its remote's (124;
    /// record 0069): its tracking ref as last fetched, or, the first time
    /// this process reads it and only where no tick of this process asked
    /// the remote, fetched as every read of a branch on its remote is
    /// (C.7; 129), as last fetched when the remote can't be read. Where the
    /// remote holds no revision of it and the host no tracking ref, it has
    /// no signer: the host's own copy is never read for one. A branch in
    /// the host's own files reads them at the revision its routing read
    /// (C.6), only where that names signers.
    ///
    /// Read again only once that revision moves, never while it stands
    /// (rule core.31). A policy refused there fails, naming the branch, the
    /// remote and the revision. Bound for guarding, it reads no key and
    /// fetches nothing.
    fn signers_of(&self, branch: &str, routed: Option<&str>) -> Result<Signers> {
        if self.guarding {
            return Ok(Signers::default());
        }
        let rev = match routed {
            Some(remote) => self.revision_on(remote, branch)?,
            None => self
                .branches
                .policy_of(branch)?
                .filter(|(_, policy)| !policy.signers.is_empty())
                .map(|(at, _)| at),
        };
        self.signers_at(branch, routed, rev)
    }

    /// `branch`'s signers as its policy at `rev` names them, none where
    /// there is no revision: the list kept for the branch while `rev` is
    /// the revision it was read at, otherwise read whole there and kept in
    /// its place (rule core.31).
    fn signers_at(&self, branch: &str, routed: Option<&str>, rev: Option<Rev>) -> Result<Signers> {
        if let Some((at, list)) = self.lists.borrow().get(branch) {
            if *at == rev {
                return Ok(list.clone());
            }
        }
        let list = match &rev {
            Some(at) => Rc::new(self.read_signers(branch, routed, at)?),
            None => Signers::default(),
        };
        self.lists
            .borrow_mut()
            .insert(branch.into(), (rev, list.clone()));
        Ok(list)
    }

    /// 124: `branch`'s revision on `remote` as [`Records::signers_of`]
    /// reads it: fetched the first time, where no tick asked the remote,
    /// and as last fetched otherwise; `None` when the host holds none.
    fn revision_on(&self, remote: &str, branch: &str) -> Result<Option<Rev>> {
        let p = self.branches.pushed_refs(remote)?;
        let first = !self.lists.borrow().contains_key(branch);
        // 129: the tick's one ask of the remote brought every branch on it,
        // so its tracking ref is the branch's revision there.
        if first && !self.asked.borrow().contains(remote) {
            match p.read(branch) {
                Ok(Some(at)) => return Ok(Some(at)),
                Ok(None) => {}
                Err(e) if read_refused(&e).is_some() => {}
                Err(e) => return Err(e),
            }
        }
        p.held(branch)
    }

    /// The public half of the key `signing` names, read once.
    fn own_key(&self, signing: &instance::Signing) -> Result<ssh_key::PublicKey> {
        if let Some(k) = self.own_key.borrow().as_ref() {
            return Ok(k.clone());
        }
        let k = ssh::private_key(&signing.key)?.public_key().clone();
        *self.own_key.borrow_mut() = Some(k.clone());
        Ok(k)
    }

    /// The keys of the signers the policy at `rev`, read whole, names for
    /// `branch`'s runs on `remote`, or in the host's own files.
    fn read_signers(
        &self,
        branch: &str,
        remote: Option<&str>,
        rev: &Rev,
    ) -> Result<BTreeMap<String, SignerKey>> {
        let mut signers = BTreeMap::new();
        let Some(bytes) = self.branches.file_at(rev, POLICY_PATH)? else {
            return Ok(signers);
        };
        self.lists_read.set(self.lists_read.get() + 1);
        let policy = Policy::parse(std::str::from_utf8(&bytes)?).with_context(|| match remote {
            Some(remote) => format!("{branch}'s policy on {remote} at {rev}"),
            None => format!("{branch}'s policy at {rev}"),
        })?;
        for (name, s) in &policy.signers {
            let key = match (&s.identity.0[..], &s.identity.1) {
                ("ssh", Setting::Text(v)) => {
                    let text = if v.split_whitespace().count() >= 2 {
                        v.clone()
                    } else {
                        let bytes = self.branches.file_at(rev, v)?.with_context(|| {
                            format!("[signers.{name}] ssh names {v}, which {rev} does not hold")
                        })?;
                        String::from_utf8(bytes)?
                    };
                    SignerKey::ssh(&text).with_context(|| format!("[signers.{name}] ssh"))?
                }
                _ => SignerKey::Keyless,
            };
            signers.insert(name.clone(), key);
        }
        Ok(signers)
    }

    /// Where this host notes that `branch`'s own records moved to its
    /// remote, and what of them it left behind.
    fn moved_path(&self, branch: &str) -> PathBuf {
        self.common
            .join("sluice-remote")
            .join("moved")
            .join(format!("{branch}.json"))
    }

    fn moved(&self, branch: &str) -> Result<Option<Moved>> {
        match std::fs::read(self.moved_path(branch)) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put_moved(&self, branch: &str, m: &Moved) -> Result<()> {
        let path = self.moved_path(branch);
        std::fs::create_dir_all(path.parent().expect("a parent"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(m)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Design D10, under the branch's lock: the first time this host routes
    /// `branch` to its remote, its own records of the branch go there as
    /// the starting state when the remote has none; when another host's
    /// already are, its queued submissions are left behind, for the round
    /// to submit again.
    fn migrate(&self, branch: &str, r: &RemoteRefs) -> Result<()> {
        if self.moved(branch)?.is_some() {
            return Ok(());
        }
        let own = self.local.read(branch)?;
        if own.version == 0 {
            return self.put_moved(branch, &Moved::default());
        }
        let theirs = r.read(branch)?;
        if theirs.version == 0 {
            let mut changes = Changes {
                put: own.files.clone(),
                ..Changes::default()
            };
            for (name, bytes) in self.local.read_all_finished(branch)? {
                changes
                    .put
                    .insert(format!("{}/{name}", sluice_seams::FINISHED), bytes);
            }
            if r.write(branch, 0, changes)? != Written::Refused {
                return self.put_moved(branch, &Moved::default());
            }
        }
        let left = own
            .files
            .iter()
            .filter(|(p, _)| p.starts_with("submissions/"))
            .filter_map(|(_, b)| {
                let v: serde_json::Value = serde_json::from_slice(b).ok()?;
                let out = matches!(
                    v["state"].as_str(),
                    Some("landed" | "withdrawn" | "failed" | "conflict")
                );
                (!out).then(|| b.clone())
            })
            .collect();
        self.put_moved(branch, &Moved { left })
    }

    /// The first remote opened, which keeps what this host has observed
    /// of the others' signs of life.
    fn shared(&self) -> Option<Rc<RemoteRefs>> {
        self.remotes.borrow().values().next().cloned()
    }
}

impl State for Records {
    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
        match self.route(branch)? {
            None => self.local.lock(branch),
            Some(r) => {
                let g = self.locks.lock(branch)?;
                if g.is_some() {
                    self.migrate(branch, &r)?;
                }
                Ok(g)
            }
        }
    }

    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>> {
        match self.route(branch)? {
            None => self.local.wait_lock(branch),
            Some(r) => {
                let g = self.locks.wait_lock(branch)?;
                self.migrate(branch, &r)?;
                Ok(g)
            }
        }
    }

    fn publish_head(&self, branch: &str, head: &Rev) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.publish_head(branch, head),
        }
    }

    fn unpublish_head(&self, branch: &str, head: &Rev) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.unpublish_head(branch, head),
        }
    }

    fn fetch_heads(&self, branch: &str) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.fetch_heads(branch),
        }
    }

    fn beat(&self, host: &str, beat: &[u8]) -> Result<()> {
        let remotes: Vec<Rc<RemoteRefs>> = self.remotes.borrow().values().cloned().collect();
        for r in remotes {
            r.beat(host, beat)?;
        }
        Ok(())
    }

    fn beats(&self) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        let mut all = BTreeMap::new();
        let remotes: Vec<Rc<RemoteRefs>> = self.remotes.borrow().values().cloned().collect();
        for r in remotes {
            all.extend(r.beats()?);
        }
        Ok(all)
    }

    fn observed_beat(&self, host: &str) -> Result<Option<(String, String)>> {
        match self.shared() {
            Some(r) => r.observed_beat(host),
            None => Ok(None),
        }
    }

    fn observe_beat(&self, host: &str, marker: &str, at: &str) -> Result<()> {
        match self.shared() {
            Some(r) => r.observe_beat(host, marker, at),
            None => Ok(()),
        }
    }

    fn publish_candidate(&self, branch: &str, id: &str, rev: &Rev) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.publish_candidate(branch, id, rev),
        }
    }

    fn unpublish_candidate(&self, branch: &str, id: &str) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.unpublish_candidate(branch, id),
        }
    }

    fn fetch_candidates(&self, branch: &str) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.fetch_candidates(branch),
        }
    }

    fn fetch_candidate(&self, branch: &str, id: &str) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.fetch_candidate(branch, id),
        }
    }

    fn observed(&self, branch: &str) -> Result<Option<(u64, String)>> {
        match self.route(branch)? {
            None => Ok(None),
            Some(r) => r.observed(branch),
        }
    }

    fn observe(&self, branch: &str, version: u64, at: &str) -> Result<()> {
        match self.route(branch)? {
            None => Ok(()),
            Some(r) => r.observe(branch, version, at),
        }
    }

    fn left_behind(&self, branch: &str) -> Result<Vec<Vec<u8>>> {
        Ok(self.moved(branch)?.map(|m| m.left).unwrap_or_default())
    }

    fn forget_left_behind(&self, branch: &str, ids: &[u64]) -> Result<()> {
        let Some(mut m) = self.moved(branch)? else {
            return Ok(());
        };
        m.left.retain(|doc| {
            let id = serde_json::from_slice::<serde_json::Value>(doc)
                .ok()
                .and_then(|v| v["id"].as_u64());
            !id.is_some_and(|id| ids.contains(&id))
        });
        self.put_moved(branch, &m)
    }

    fn read(&self, branch: &str) -> Result<Snapshot> {
        match self.route(branch)? {
            None => self.local.read(branch),
            Some(r) => r.read(branch),
        }
    }

    /// 129: each remote that may hold a branch's state asked once, for
    /// every branch's state and every host's sign of life, under the retry
    /// policy (rule core.29). A refusal marks every branch on that remote
    /// as last fetched in the remote's words, as a refused read of one
    /// does (rule core.28), and asks the next remote all the same.
    fn refresh(&self) -> Result<()> {
        for remote in self.signed_remotes()? {
            let p = self.branches.pushed_refs(&remote)?;
            let refused = match super::remote_refs::ask(&p) {
                Ok(()) => None,
                Err(e) => Some(read_refused(&e).cloned().ok_or(e)?),
            };
            self.asked.borrow_mut().insert(remote.clone());
            for (branch, _) in super::remote_refs::fetched_branches(&p)? {
                // A branch whose policy cannot be read is that branch's
                // alone, said wherever it is read (26).
                let _ = self.route(&branch);
            }
            let opened = self.remotes.borrow().get(&remote).cloned();
            if let Some(r) = opened {
                r.asked(refused.as_ref())?;
            }
        }
        Ok(())
    }

    fn read_held(&self, branch: &str) -> Result<Snapshot> {
        match self.route(branch)? {
            None => self.local.read_held(branch),
            Some(r) => r.read_held(branch),
        }
    }

    /// Every remote that may hold a branch's state is read as asked, from
    /// what the host process's last ask fetched, and the remotes opened
    /// now or later fetch no signs of life of their own (129; design D16).
    fn hold(&self) {
        self.held.set(true);
        let Ok(remotes) = self.signed_remotes() else {
            return;
        };
        self.asked.borrow_mut().extend(remotes);
        for r in self.remotes.borrow().values() {
            r.hold();
        }
    }

    fn stale(&self, branch: &str) -> Option<ReadRefused> {
        let remote = self.routes.borrow().get(branch).cloned().flatten()?;
        let r = self.remotes.borrow().get(&remote).cloned()?;
        r.stale(branch)
    }

    fn refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        match self.route(branch)? {
            None => self.local.refusal(branch),
            Some(r) => r.refusal(branch),
        }
    }

    fn keep_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        match self.route(branch)? {
            None => self.local.keep_refusal(branch, doc),
            Some(r) => r.keep_refusal(branch, doc),
        }
    }

    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
        match self.route(branch)? {
            None => self.local.read_finished(branch, name),
            Some(r) => r.read_finished(branch, name),
        }
    }

    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        match self.route(branch)? {
            None => self.local.read_all_finished(branch),
            Some(r) => r.read_all_finished(branch),
        }
    }

    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written> {
        match self.route(branch)? {
            None => self.local.write(branch, read_at, changes),
            Some(r) => r.write(branch, read_at, changes),
        }
    }

    /// Every branch with records: this host's own, and every branch whose
    /// state is on one of the repository's remotes: as a tick's ask of the
    /// remote fetched them, read with no process (129), or, before any
    /// tick asked it, as the remote lists them, read once (80).
    fn branches(&self) -> Result<Vec<String>> {
        let mut all = self.local.branches()?;
        let asked: Vec<String> = self.asked.borrow().iter().cloned().collect();
        let mut found = Vec::new();
        for remote in &asked {
            let p = self.branches.pushed_refs(remote)?;
            found.extend(
                super::remote_refs::fetched_branches(&p)?
                    .into_iter()
                    .map(|(b, _)| b),
            );
        }
        if self.listed.borrow().is_none() {
            let mut listed = BTreeMap::new();
            for remote in self.signed_remotes()? {
                if asked.contains(&remote) {
                    continue;
                }
                let p = self.branches.pushed_refs(&remote)?;
                let out = p.remote_git(&["ls-remote", "--refs"], &["refs/sluice/state/*"])?;
                if !out.status.success() {
                    // A remote out of reach lists nothing here; a branch
                    // already routed is read as last fetched (80).
                    continue;
                }
                let on: Vec<String> = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|l| l.split('\t').nth(1))
                    .filter_map(|r| r.strip_prefix("refs/sluice/state/"))
                    .map(str::to_string)
                    .collect();
                listed.insert(remote, on);
            }
            *self.listed.borrow_mut() = Some(listed);
        }
        for (remote, on) in self.listed.borrow().iter().flatten() {
            if !asked.contains(remote) {
                found.extend(on.iter().cloned());
            }
        }
        for b in found {
            if !all.contains(&b) {
                all.push(b);
            }
        }
        all.sort();
        Ok(all)
    }

    /// The count of the records that hold `branch`: its remote's where it
    /// is routed there, the host's own where its records are the host's,
    /// whatever other remote this binding has opened, so no new number
    /// repeats one the branch already holds (26; record 0114; rule
    /// core.41).
    fn allocate_id(&self, branch: &str) -> Result<u64> {
        match self.route(branch)? {
            Some(r) => r.allocate_id(branch),
            None => self.local.allocate_id(branch),
        }
    }

    /// Of this host's own results, and the runs on every remote opened,
    /// each signer's verified with the key `branch`'s own policy lists for
    /// it at its revision and read for no signer it does not name
    /// ([`Records::signers_of`]; 78; rule core.22), the lines `ask` names:
    /// from the host process's index in the host process, asked of it from
    /// a command, and read here otherwise (design D17).
    fn results_for(&self, branch: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        match self.results_from {
            ResultsFrom::Here => self.results_here(branch, ask),
            ResultsFrom::Indexed => self.results_indexed(branch, ask),
            ResultsFrom::Asked => {
                let asked = crate::host_process::client::Client::host()
                    .and_then(|c| c.results(&self.common, branch, ask));
                match asked {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        crate::telemetry::warn(&format!(
                            "sluice: warning: sluice's host process could not be asked for {branch}'s results, so this command reads them itself: {e:#}"
                        ));
                        self.results_here(branch, ask)
                    }
                }
            }
        }
    }

    /// 78, 87: whether `branch`'s own policy at `at`, the list its runs are
    /// verified with there ([`Records::signers_at`]), lists the key this
    /// host signs with for the signer it signs as; read with no fetch, at
    /// the revision the round read. A host that signs nothing, or a
    /// binding for guarding, has no run that counts.
    fn verifies_own(&self, branch: &str, at: &Rev) -> Result<bool> {
        let Some(signing) = self.signing.as_ref().filter(|_| !self.guarding) else {
            return Ok(false);
        };
        let routed = self.route(branch)?;
        let signers = self.signers_at(
            branch,
            routed.as_ref().map(|r| r.remote()),
            Some(at.clone()),
        )?;
        let Some(listed) = signers.get(&signing.signer) else {
            return Ok(false);
        };
        Ok(listed.lists(&self.own_key(signing)?))
    }

    fn record_results(&self, lines: &[u8]) -> Result<()> {
        let lines: Vec<ResultLine> = parse_lines(lines)?;
        let mut by: BTreeMap<Option<String>, Vec<u8>> = BTreeMap::new();
        for l in &lines {
            let route = self.route(&l.branch)?.map(|r| r.remote().to_string());
            by.entry(route).or_default().extend(to_line(l));
        }
        for (route, bytes) in by {
            match route {
                None => self.local.record_results(&bytes)?,
                Some(remote) => {
                    let r = self.remotes.borrow()[&remote].clone();
                    r.record_results(&bytes)?;
                }
            }
        }
        Ok(())
    }

    fn set_asides(&self) -> Result<Vec<u8>> {
        let mut bytes = self.local.set_asides()?;
        let remotes: Vec<Rc<RemoteRefs>> = self.remotes.borrow().values().cloned().collect();
        for r in remotes {
            bytes.extend(r.set_asides()?);
        }
        Ok(bytes)
    }

    /// Kept where the result's branch keeps its records, so every host
    /// that reads the result reads it set aside (rule core.11).
    fn set_aside(&self, line: &[u8]) -> Result<()> {
        let aside: Vec<SetAside> = parse_lines(line)?;
        let branch = aside.first().map(|a| a.branch.as_str()).unwrap_or_default();
        match self.route(branch)? {
            None => self.local.set_aside(line),
            Some(r) => r.set_aside(line),
        }
    }

    fn keep_logs(&self, branch: &str, candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>> {
        match self.route(branch)? {
            None => self.local.keep_logs(branch, candidate, logs),
            Some(r) => r.keep_logs(branch, candidate, logs),
        }
    }

    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
        match self.route(branch)? {
            None => self.local.progress(branch),
            Some(r) => r.progress(branch),
        }
    }

    fn put_progress(
        &self,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        match self.route(branch)? {
            None => self.local.put_progress(branch, candidate, tasks),
            Some(r) => r.put_progress(branch, candidate, tasks),
        }
    }
}
