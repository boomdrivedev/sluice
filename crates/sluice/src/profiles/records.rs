//! The records seam as an instance binds it (C.6; ADR 0037; record 0122):
//! a branch to the home its records name in `home.json`, `local files` for
//! the host's own and `remote refs` for a remote's, and only a branch with
//! no home anywhere by its policy, a `remote` and signers to that remote
//! and every other to `local files`. The results, which name no branch, go
//! to the remote once a branch has been routed there; a branch's next
//! number comes from the count of the records that hold it, so a remote's
//! are numbered once across every host and the host's own once on the
//! host (record 0114).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use sluice_core::home::{HomeStore, Owed};
use sluice_core::policy::{Policy, POLICY_PATH};
use sluice_core::records::{
    parse_lines, to_document, to_line, HomeDoc, HomeState, ResultLine, SetAside, HERE, HOME_FILE,
};
use sluice_seams::{
    read_refused, Branch, Carried, Changes, Guard, ReadRefused, ResultAsk, Rev, Setting, Snapshot,
    State, TaskLog, TaskProgress, Written,
};

use super::branches::Branches;
use super::local_files::LocalFiles;
use super::remote_refs::{RemoteRefs, SignerKey, Signing};
use crate::instance;
use crate::signing::ssh;

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
    /// Record 0122 (design D2): where each branch was last routed within
    /// this pass, read again at every lock, read and write and forgotten
    /// at the next pass.
    routes: RefCell<BTreeMap<String, Route>>,
    /// Where each branch was last read from, so a write is refused where
    /// a move took its records elsewhere since (62).
    read_from: RefCell<BTreeMap<String, Option<String>>>,
    /// 51, record 0122 (design D4): this host as its peers name it, which
    /// a submission it made names as its host.
    host: String,
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

/// Record 0122 (design D2): where a branch's records are.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Route {
    /// The remote whose refs hold them; `None` for the host's own files.
    remote: Option<String>,
    /// Where no home holds them anywhere: routed by the policy at this
    /// revision, which the first write's home names.
    by_policy: Option<Option<Rev>>,
}

/// Record 0122 (design D3): the host's own files as one home of a branch,
/// for the core's carry.
struct OwnHome<'a> {
    local: &'a LocalFiles,
    branch: &'a str,
}

impl HomeStore for OwnHome<'_> {
    fn name(&self) -> String {
        HERE.into()
    }

    fn read(&self) -> Result<Snapshot> {
        self.local.read(self.branch)
    }

    fn read_finished(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.local.read_finished(self.branch, name)
    }

    fn write(&self, read_at: u64, changes: Changes) -> Result<Written> {
        self.local.write(self.branch, read_at, changes)
    }

    fn raise_past(&self, number: u64) -> Result<()> {
        self.local.raise_ids_past(number)
    }

    fn next_number(&self) -> Result<u64> {
        self.local.allocate_id(self.branch)
    }
}

/// Record 0122 (design D3): a remote's refs as one home of a branch, for
/// the core's carry, its state written leased on the version read.
struct RemoteHome {
    refs: Rc<RemoteRefs>,
    branch: String,
}

impl HomeStore for RemoteHome {
    fn name(&self) -> String {
        self.refs.remote().to_string()
    }

    fn read(&self) -> Result<Snapshot> {
        self.refs.read(&self.branch)
    }

    fn read_finished(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.refs.read_finished(&self.branch, name)
    }

    fn write(&self, read_at: u64, changes: Changes) -> Result<Written> {
        self.refs.write(&self.branch, read_at, changes)
    }

    fn raise_past(&self, number: u64) -> Result<()> {
        self.refs.raise_ids_past(number)
    }

    fn next_number(&self) -> Result<u64> {
        self.refs.allocate_id(&self.branch)
    }
}

/// Both of a branch's locks on the host, held together: its own files'
/// and its remote's, so whichever home a move leaves its records in, the
/// round that moved them still holds the lock they are written under
/// (rule core.14).
struct BothLocks<'a> {
    _own: Box<dyn Guard + 'a>,
    _remote: Box<dyn Guard + 'a>,
}

impl Guard for BothLocks<'_> {}

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
            read_from: RefCell::default(),
            host: super::checks::hostname(),
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

    /// The same binding, for the host its peers call `name` (51; record
    /// 0122, design D4).
    pub fn as_host(self, name: &str) -> Self {
        Records {
            host: name.into(),
            ..self
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
        self.routes.borrow_mut().clear();
    }

    /// The host's own records, for what only this host keeps.
    pub fn local(&self) -> &LocalFiles {
        &self.local
    }

    /// The `remote refs` profile `branch` is routed to, or `None` for
    /// local files, read from its records (record 0122; C.6; design D2):
    /// this host's own `home.json` of it, open or moving to its own files
    /// and closed to the home its pointer names; where it has none, the
    /// state's on each remote that may hold it, as last fetched (129), one
    /// open or moving; and only where neither holds a home, the policy the
    /// routing reads, its remote's when it names signers and a remote for
    /// it (27), each revision's policy read only as far as that needs (124;
    /// record 0069). A remote home closed toward the hosts' own files routes
    /// to this host's own (design D4). Read afresh at every call and kept
    /// for nothing past the pass ([`Records::next_pass`]).
    ///
    /// The signers its runs are verified by are not the routing's: they
    /// are the branch's own, read from its revision on the remote, never
    /// from the host's own copy nor from another branch's policy
    /// ([`Records::signers_of`]; 56, 78, 124; rule core.22).
    pub fn route(&self, branch: &str) -> Result<Option<Rc<RemoteRefs>>> {
        let route = self.find_route(branch)?;
        let remote = match &route.remote {
            Some(r) => Some(self.open(r)?),
            None => None,
        };
        self.routes.borrow_mut().insert(branch.into(), route);
        Ok(remote)
    }

    /// Design D2: where `branch`'s records are, as [`Records::route`]
    /// reads it.
    fn find_route(&self, branch: &str) -> Result<Route> {
        let here = Route {
            remote: None,
            by_policy: None,
        };
        let on = |remote: &str| Route {
            remote: Some(remote.to_string()),
            by_policy: None,
        };
        let remotes = self.home_remotes(branch)?;
        if let Some(own) = self.own_home(branch)? {
            match own.state {
                HomeState::Moving => return Ok(here),
                HomeState::Open => {
                    // 26: this host's own queue open while a remote's that
                    // came from it is live: one queue in two homes.
                    for r in &remotes {
                        let split = self.held_home(r, branch)?.is_some_and(|h| {
                            h.is_live()
                                && h.came_from.as_ref().is_some_and(|c| {
                                    c.home == HERE && c.host.as_deref() == Some(self.host.as_str())
                                })
                        });
                        if split {
                            return Err(sluice_core::home::two_homes(branch, HERE, r));
                        }
                    }
                    return Ok(here);
                }
                HomeState::Closed => {
                    let to = own.closed_to().unwrap_or(HERE).to_string();
                    if to == HERE {
                        return Ok(here);
                    }
                    return Ok(match self.held_home(&to, branch)? {
                        Some(h) if h.closed_to() == Some(HERE) => here,
                        _ => on(&to),
                    });
                }
            }
        }
        for r in &remotes {
            match self.held_home(r, branch)? {
                Some(h) if h.is_live() => return Ok(on(r)),
                Some(h) if h.closed_to() == Some(HERE) => return Ok(here),
                _ => {}
            }
        }
        Ok(match self.branches.policy_of(branch)? {
            Some((at, policy)) if !policy.signers.is_empty() => match policy.remote_of(branch)? {
                Some(remote) => Route {
                    remote: Some(remote),
                    by_policy: Some(Some(at)),
                },
                None => Route {
                    remote: None,
                    by_policy: Some(Some(at)),
                },
            },
            Some((at, _)) => Route {
                remote: None,
                by_policy: Some(Some(at)),
            },
            None => Route {
                remote: None,
                by_policy: Some(None),
            },
        })
    }

    /// Record 0122 (design D2): the remotes that may hold `branch`'s home:
    /// its remote as the policy the routing reads names it (27), signers or
    /// none, and every remote that may hold a branch's state
    /// ([`Records::signed_remotes`]). A policy that cannot be read names
    /// none here; its refusal is said where the branch is routed by it.
    fn home_remotes(&self, branch: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if let Ok(Some((_, policy))) = self.branches.policy_of(branch) {
            if let Ok(Some(remote)) = policy.remote_of(branch) {
                out.push(remote);
            }
        }
        for r in self.signed_remotes()? {
            if !out.contains(&r) {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Record 0122 (design D1): `branch`'s home document in this host's own
    /// files, as they read now.
    fn own_home(&self, branch: &str) -> Result<Option<HomeDoc>> {
        let read = self.local.read(branch)?;
        match read.files.get(HOME_FILE) {
            Some(bytes) => Ok(Some(serde_json::from_slice(bytes).with_context(|| {
                format!("reading {branch}'s {HOME_FILE} in this host's own files")
            })?)),
            None => Ok(None),
        }
    }

    /// Record 0122 (design D2): `branch`'s home document on `remote`, as
    /// this host last fetched it, with no fetch.
    fn held_home(&self, remote: &str, branch: &str) -> Result<Option<HomeDoc>> {
        self.open(remote)?.held_home(branch)
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

    /// The first remote opened, which keeps what this host has observed
    /// of the others' signs of life.
    fn shared(&self) -> Option<Rc<RemoteRefs>> {
        self.remotes.borrow().values().next().cloned()
    }
}

impl State for Records {
    /// Both of the branch's locks, its own files' first: a move between
    /// its homes stays under the lock its round took (rule core.14). Its
    /// route is read, so a branch refused by its homes is refused here.
    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
        self.route(branch)?;
        let Some(own) = self.local.lock(branch)? else {
            return Ok(None);
        };
        let Some(remote) = self.locks.lock(branch)? else {
            return Ok(None);
        };
        Ok(Some(Box::new(BothLocks {
            _own: own,
            _remote: remote,
        })))
    }

    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>> {
        self.route(branch)?;
        let own = self.local.wait_lock(branch)?;
        let remote = self.locks.wait_lock(branch)?;
        Ok(Box::new(BothLocks {
            _own: own,
            _remote: remote,
        }))
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

    /// Record 0122 (design D3, D4; C.6): the core's one carry, between the
    /// host's own files and a remote's refs. Where the home the branch is
    /// routed to is moving, which only the round that keeps it asks, its
    /// queue goes to the home the move names, every live submission of it,
    /// or, coming home from a remote, those this host made; where it is not,
    /// what this host made left in a remote home closed toward the hosts'
    /// own files comes into its own. Each write reads its target first, a
    /// remote's leased on the version read (62).
    fn carry(&self, branch: &str) -> Result<Carried> {
        let routed = self.route(branch)?;
        let home = match &routed {
            None => self.own_home(branch)?,
            Some(r) => r.held_home(branch)?,
        };
        let own = || OwnHome {
            local: &self.local,
            branch,
        };
        let remote = |name: &str| -> Result<RemoteHome> {
            Ok(RemoteHome {
                refs: self.open(name)?,
                branch: branch.to_string(),
            })
        };
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let done = || Ok(());
        match home
            .as_ref()
            .and_then(|h| h.moving.as_ref().filter(|_| h.state == HomeState::Moving))
        {
            Some(moving) => {
                let chosen_at = moving.chosen_at.clone();
                let from: Box<dyn HomeStore + '_> = match &routed {
                    None => Box::new(own()),
                    Some(r) => Box::new(remote(r.remote())?),
                };
                let (to, owed): (Box<dyn HomeStore + '_>, Owed) = match moving.to.as_str() {
                    HERE => (Box::new(own()), Owed::Mine),
                    name => (Box::new(remote(name)?), Owed::All),
                };
                sluice_core::home::carry(
                    branch,
                    from.as_ref(),
                    to.as_ref(),
                    &self.host,
                    owed,
                    chosen_at,
                    &now,
                    &done,
                )
            }
            None if routed.is_none() => {
                for r in self.home_remotes(branch)? {
                    let closed_here = self
                        .held_home(&r, branch)?
                        .is_some_and(|h| h.closed_to() == Some(HERE));
                    if closed_here {
                        return sluice_core::home::carry(
                            branch,
                            &remote(&r)?,
                            &own(),
                            &self.host,
                            Owed::Mine,
                            None,
                            &now,
                            &done,
                        );
                    }
                }
                Ok(Carried::default())
            }
            None => Ok(Carried::default()),
        }
    }

    fn read(&self, branch: &str) -> Result<Snapshot> {
        let routed = self.route(branch)?;
        self.read_from.borrow_mut().insert(
            branch.into(),
            routed.as_ref().map(|r| r.remote().to_string()),
        );
        match routed {
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
        let remote = self.routes.borrow().get(branch)?.remote.clone()?;
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

    fn intake_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        match self.route(branch)? {
            None => self.local.intake_refusal(branch),
            Some(r) => r.intake_refusal(branch),
        }
    }

    fn keep_intake_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        match self.route(branch)? {
            None => self.local.keep_intake_refusal(branch, doc),
            Some(r) => r.keep_intake_refusal(branch, doc),
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

    /// 62, record 0122 (design D2): written where the branch is routed
    /// now, refused where a move took its records from the home they were
    /// read in. The first write into a home that holds no `home.json`, a
    /// branch routed by its policy, writes it, open, naming the home and
    /// the revision whose policy chose it.
    fn write(&self, branch: &str, read_at: u64, mut changes: Changes) -> Result<Written> {
        let routed = self.route(branch)?;
        let now = routed.as_ref().map(|r| r.remote().to_string());
        if let Some(read) = self.read_from.borrow().get(branch) {
            if *read != now {
                return Ok(Written::Refused);
            }
        }
        let by_policy = self
            .routes
            .borrow()
            .get(branch)
            .and_then(|r| r.by_policy.clone());
        if let Some(chosen_at) = by_policy {
            if !changes.put.contains_key(HOME_FILE) {
                let at = now.as_deref().unwrap_or(HERE);
                changes
                    .put
                    .insert(HOME_FILE.into(), to_document(&HomeDoc::open(at, chosen_at)));
            }
        }
        match routed {
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
