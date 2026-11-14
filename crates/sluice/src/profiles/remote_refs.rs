//! The records seam's `remote refs` profile (C.6; ADR 0037): a branch's
//! state on the remote its policy names, for every host that submits to
//! it (80, 82).
//!
//! ```text
//! refs/sluice/state/<branch>         one root commit: the branch's documents
//!                                    as `local files` lays them out, and
//!                                    `version`; written by a push leased on
//!                                    the commit read (62, 63)
//! refs/sluice/runs/<signer>          each signer's signed run statements,
//!                                    only added to (109)
//! refs/sluice/ids                    the next submission id
//! refs/sluice/logs/<branch>/<cand>   a failed candidate's task outputs (20)
//! ```
//!
//! What a host keeps of its own, its runs' tasks (ADR 0028) and the logs
//! it read, is in `<common>/sluice-remote/<remote>/`, a `local files` of
//! its own. A replay is recorded there too: it is no one's observation
//! (core.9), so no signer signs it. Beside it, what the host observed of
//! each branch (73) and a refusal of its reads that stands (133), under
//! `observed/` and `refused/`. Its locks are the host's one lock of each
//! branch on any remote, under `<common>/sluice-remote/locks/`, held by
//! the records seam that routes the branch here, and a run's tasks are
//! kept only while that lock is held (rule core.14).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::rc::Rc;

use anyhow::{anyhow, bail, Context, Result};
use sluice_core::records::{parse_lines, to_line, ResultKind, ResultLine, RESULT_SCHEMA};
use sluice_seams::{
    read_refused, Changes, Enforced, Guard, ReadRefused, ResultAsk, Rev, Snapshot, State, TaskLog,
    TaskProgress, Written, FINISHED, KEPT_LOGS,
};

use super::local_files::LocalFiles;
use super::pushed_refs::{command, Pushed, PushedRefs};
use crate::signing::dsse::Envelope;
use crate::signing::ssh;
use crate::signing::statement::{Claims, Held, Run, RunResult, Statement};

/// Where the results set aside are kept, one file each (could-not-run-1
/// 116).
const SET_ASIDE: &str = "refs/sluice/set-aside";

/// Every head submitted to the remote, by its revision (82).
const HEADS: &str = "refs/sluice/heads/*";

/// The file of a branch's state commit holding its version.
const VERSION: &str = "version";

/// How many times an add to a shared ref is tried before it fails: an add
/// refused because another host of the same signer added first is put on
/// top and pushed again (design D2), which decides nothing.
const ADD_TRIES: usize = 8;

/// A signer the branch's policy names, as this profile verifies it (78).
#[derive(Clone, Debug)]
pub enum SignerKey {
    /// Its allowed-signers lines, and a digest of them that tells this
    /// key from another the same signer is listed with elsewhere.
    Ssh {
        allowed: Vec<ssh::Allowed>,
        digest: String,
    },
    /// A forge job's keyless identity, verified by the forge's profile.
    Keyless,
}

impl SignerKey {
    /// The allowed-signers lines of `text`, `ssh-keygen`'s form.
    pub fn ssh(text: &str) -> Result<SignerKey> {
        let allowed = ssh::allowed(text)?;
        let lines: String = allowed
            .iter()
            .map(|a| {
                let at = a.key.fingerprint(ssh_key::HashAlg::Sha256);
                format!("{} {at}\n", a.principal)
            })
            .collect();
        Ok(SignerKey::Ssh {
            digest: sha256_hex(lines.as_bytes()),
            allowed,
        })
    }

    /// Whether `key` is among those listed: a signature it makes verifies
    /// (78, 87).
    pub fn lists(&self, key: &ssh_key::PublicKey) -> bool {
        match self {
            SignerKey::Ssh { allowed, .. } => {
                allowed.iter().any(|a| a.key.key_data() == key.key_data())
            }
            SignerKey::Keyless => false,
        }
    }

    /// What names this key among every key a signer is listed with.
    fn digest(&self) -> &str {
        match self {
            SignerKey::Ssh { digest, .. } => digest,
            SignerKey::Keyless => "keyless",
        }
    }
}

/// This host's signer and its key (78; the host's `[signing]`).
pub struct Signing {
    pub signer: String,
    pub key: ssh_key::PrivateKey,
}

/// A branch's state as last read: its version, the commit holding it, and
/// every file of its tree by path, with the blob each is.
#[derive(Clone, Debug, Default)]
struct Read {
    version: u64,
    commit: Option<String>,
    files: BTreeMap<String, (String, Vec<u8>)>,
}

pub struct RemoteRefs {
    pushed: Rc<PushedRefs>,
    own: LocalFiles,
    /// The host's locks of the branches on its remotes, one each whatever
    /// remote holds it (rule core.14).
    locks: Rc<LocalFiles>,
    signing: Option<Signing>,
    read: RefCell<BTreeMap<String, Read>>,
    /// The branches whose last read could not reach the remote, and why
    /// (80, 133).
    stale: RefCell<BTreeMap<String, ReadRefused>>,
    /// Statements already verified, by their blob, the signer they were
    /// read for and the digest of the key they were verified with, so a
    /// round verifies only what it has not seen and a statement verified
    /// with one branch's key is never read as verified with another's
    /// (78; rule core.22).
    verified: RefCell<BTreeMap<(String, String, String), Option<Vec<ResultLine>>>>,
    /// 129: a tick's ask fetched the hosts' signs of life since
    /// [`State::beats`] last answered: the next answers from it.
    asked: std::cell::Cell<bool>,
    /// 129 (design D16): every answer reads what the host process's last
    /// ask fetched, and none fetches ([`RemoteRefs::hold`]).
    held: std::cell::Cell<bool>,
}

impl RemoteRefs {
    /// The remote `pushed` names. It keeps no signer's key: a branch's
    /// runs are verified with the list its own policy names, handed to
    /// [`RemoteRefs::results_with`] (78; rule core.22).
    pub fn new(common: &Path, pushed: Rc<PushedRefs>, signing: Option<Signing>) -> Self {
        let own = LocalFiles::at(common.join("sluice-remote").join(pushed.remote()));
        let locks = Rc::new(LocalFiles::at(common.join("sluice-remote").join("locks")));
        RemoteRefs {
            pushed,
            own,
            locks,
            signing,
            read: RefCell::default(),
            stale: RefCell::default(),
            verified: RefCell::default(),
            asked: std::cell::Cell::new(false),
            held: std::cell::Cell::new(false),
        }
    }

    /// 129 (design D16): from now on the hosts' signs of life are read as
    /// the host process's last ask fetched them, with no fetch: a one-shot
    /// read's.
    pub fn hold(&self) {
        self.held.set(true);
    }

    /// The same profile, its branches locked through `locks`, the lock the
    /// records seam takes for every branch it routes to a remote: a round
    /// keeps its tasks under the lock it took (rule core.14).
    pub fn sharing_locks(self, locks: Rc<LocalFiles>) -> Self {
        RemoteRefs { locks, ..self }
    }

    /// 129: a tick's ask of the remote was made: `refused` when it was
    /// refused. Every branch on it, as fetched, is read as last fetched
    /// with the refusal as its stale read, or as fetched now with none,
    /// and the next [`State::beats`] answers from what was fetched.
    pub fn asked(&self, refused: Option<&ReadRefused>) -> Result<()> {
        self.asked.set(true);
        let mut stale = self.stale.borrow_mut();
        for (branch, _) in fetched_branches(&self.pushed)? {
            match refused {
                Some(r) => stale.insert(branch, r.clone()),
                None => stale.remove(&branch),
            };
        }
        Ok(())
    }

    pub fn remote(&self) -> &str {
        self.pushed.remote()
    }

    /// The results each signer in `signers` signed, each verified with the
    /// key `signers` lists for it and with no other, and this host's own
    /// replays (core.9). `signers` is the list the policy of the branch
    /// whose gate reads them names (78; rule core.22).
    /// Where this host's own replays on the remote are kept (design D17).
    pub fn own_results_path(&self) -> PathBuf {
        self.own.results_path()
    }

    /// `signer`'s runs on the remote as this host last fetched them, read
    /// for a branch whose policy lists `key` for it: what the host
    /// process's results index reads (design D17; rule core.22).
    pub fn signer_ref<'a>(&'a self, signer: &'a str, key: &'a SignerKey) -> FetchedRuns<'a> {
        FetchedRuns {
            refs: self,
            signer,
            key,
        }
    }

    pub fn results_with(&self, signers: &BTreeMap<String, SignerKey>) -> Result<Vec<u8>> {
        let mut bytes = self.own.results_log()?;
        for (signer, key) in signers {
            for l in self.signed(signer, key)? {
                bytes.extend(to_line(&l));
            }
        }
        Ok(bytes)
    }

    fn git(&self, args: &[&str]) -> Result<Output> {
        command(self.pushed.git_dir(), args)
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))
    }

    fn git_ok(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = self.git(args)?;
        if !out.status.success() {
            bail!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }

    /// Where a remote ref is fetched to here.
    fn fetched(&self, name: &str) -> String {
        let rest = name.strip_prefix("refs/sluice/").unwrap_or(name);
        format!("refs/sluice-fetched/{}/{rest}", self.remote())
    }

    /// 82: the refspec that fetches every head submitted to the remote.
    fn heads_spec(&self) -> String {
        format!("+{HEADS}:refs/sluice-fetched/{}/heads/*", self.remote())
    }

    /// 82: the refspec that fetches `branch`'s combinations open to claims.
    fn combinations_spec(&self, branch: &str) -> String {
        let remote = self.remote();
        format!("+refs/sluice/combinations/{branch}/*:refs/sluice-fetched/{remote}/combinations/{branch}/*")
    }

    /// Fetch `name` from the remote; `None` when it has none.
    fn fetch(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .pushed
            .fetch_ref(name, &self.fetched(name))?
            .map(|r| r.0))
    }

    /// Every file of a commit's tree, by path, with its blob, read with
    /// no process (129).
    fn tree(&self, commit: &str) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        self.pushed.local().tree_files(commit)
    }

    /// `branch`'s state at `commit`, as kept when it was last read there.
    fn state_at(&self, branch: &str, name: &str, commit: Option<String>) -> Result<Read> {
        let kept = self.read.borrow().get(branch).cloned();
        let read = match (commit, kept) {
            (None, _) => Read::default(),
            (Some(commit), Some(r)) if r.commit.as_deref() == Some(commit.as_str()) => r,
            (Some(commit), _) => {
                let files = self.tree(&commit)?;
                let version = match files.get(VERSION) {
                    Some((_, v)) => std::str::from_utf8(v)?
                        .trim()
                        .parse()
                        .with_context(|| format!("{name}'s version"))?,
                    None => bail!("{name} on {} has no version", self.remote()),
                };
                Read {
                    version,
                    commit: Some(commit),
                    files,
                }
            }
        };
        self.read.borrow_mut().insert(branch.into(), read.clone());
        Ok(read)
    }

    /// A blob of `bytes` in the repository.
    fn blob(&self, bytes: &[u8]) -> Result<String> {
        let mut child = command(self.pushed.git_dir(), &["hash-object", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("running git hash-object")?;
        child.stdin.take().expect("piped").write_all(bytes)?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("git hash-object failed");
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    }

    /// A commit of `files` (path to blob), with `parent` where there is
    /// one: a tree built in an index of its own, never the repository's.
    fn commit(
        &self,
        files: &BTreeMap<String, String>,
        parent: Option<&str>,
        message: &str,
    ) -> Result<String> {
        let index = tempfile_path(self.pushed.git_dir(), "index")?;
        let mut info = String::new();
        for (path, oid) in files {
            info.push_str(&format!("100644 {oid}\t{path}\n"));
        }
        let mut child = command(
            self.pushed.git_dir(),
            &["update-index", "--add", "--index-info"],
        )
        .env("GIT_INDEX_FILE", &index)
        .stdin(Stdio::piped())
        .spawn()
        .context("running git update-index")?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(info.as_bytes())?;
        if !child.wait()?.success() {
            let _ = std::fs::remove_file(&index);
            bail!("git update-index failed");
        }
        let tree = command(self.pushed.git_dir(), &["write-tree"])
            .env("GIT_INDEX_FILE", &index)
            .output()?;
        let _ = std::fs::remove_file(&index);
        if !tree.status.success() {
            bail!("git write-tree failed");
        }
        let tree = String::from_utf8(tree.stdout)?.trim().to_string();
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
        if let Some(p) = parent {
            args.extend(["-p", p]);
        }
        let out = command(self.pushed.git_dir(), &args)
            .env("GIT_AUTHOR_NAME", "sluice")
            .env("GIT_AUTHOR_EMAIL", "sluice@localhost")
            .env("GIT_COMMITTER_NAME", "sluice")
            .env("GIT_COMMITTER_EMAIL", "sluice@localhost")
            .output()?;
        if !out.status.success() {
            bail!(
                "git commit-tree: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    }

    /// Push `spec`, setting the remote's ref `name` to `to` (`None`: a
    /// removal), with `lease` when there is one, under the retry policy
    /// (134): tried again only when the remote never judged it, and read
    /// back when its answer was lost (122).
    fn push(
        &self,
        lease: Option<&str>,
        spec: &str,
        name: &str,
        to: Option<&str>,
    ) -> Result<Pushed> {
        let mut args = vec!["push", "--porcelain", "--no-verify"];
        args.extend(lease);
        self.pushed.push(&args, spec, name, to)
    }

    /// Push `commit` to `name`, leased on `expected` (`None`: the remote
    /// has no such ref). `Ok(false)` when the lease failed.
    fn push_leased(&self, commit: &str, name: &str, expected: Option<&str>) -> Result<bool> {
        let lease = format!("--force-with-lease={name}:{}", expected.unwrap_or(""));
        let spec = format!("{commit}:{name}");
        let pushed = self.push(Some(&lease), &spec, name, Some(commit))?;
        if let Pushed::Refused(out) = &pushed {
            let report = String::from_utf8_lossy(&out.stdout);
            if report.contains("stale info")
                || report.contains("fetch first")
                || report.contains("rejected")
            {
                return Ok(false);
            }
        }
        match pushed.why() {
            None => Ok(true),
            Some(why) => bail!("pushing {name} to {} failed: {why}", self.remote()),
        }
    }

    /// Remove `name` from the remote.
    fn delete(&self, name: &str) -> Result<()> {
        let spec = format!(":{name}");
        if let Some(why) = self.push(None, &spec, name, None)?.why() {
            bail!("removing {name} from {} failed: {why}", self.remote());
        }
        Ok(())
    }

    /// Remove `name` from the remote when it holds it.
    fn delete_if_present(&self, name: &str) -> Result<()> {
        let spec = format!(":{name}");
        let pushed = self.push(None, &spec, name, None)?;
        if let Pushed::Refused(out) = &pushed {
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if said.contains("does not exist") {
                return Ok(());
            }
        }
        match pushed.why() {
            None => Ok(()),
            Some(why) => bail!("removing {name} from {} failed: {why}", self.remote()),
        }
    }

    /// Add `files` to the ref `name`, on top of whatever it holds, again
    /// on top of what another host added first (design D2).
    fn add(&self, name: &str, files: &BTreeMap<String, Vec<u8>>, message: &str) -> Result<()> {
        let mut blobs = BTreeMap::new();
        for (path, bytes) in files {
            blobs.insert(path.clone(), self.blob(bytes)?);
        }
        for _ in 0..ADD_TRIES {
            let tip = self.fetch(name)?;
            let mut all: BTreeMap<String, String> = match &tip {
                Some(c) => self
                    .tree(c)?
                    .into_iter()
                    .map(|(p, (oid, _))| (p, oid))
                    .collect(),
                None => BTreeMap::new(),
            };
            all.extend(blobs.clone());
            let commit = self.commit(&all, tip.as_deref(), message)?;
            if self.push_leased(&commit, name, tip.as_deref())? {
                // What was pushed is read here as fetched, so the host
                // process's index reads a round's own runs with no fetch
                // (design D17).
                self.pushed
                    .local()
                    .write_ref(&self.fetched(name), &Rev(commit.clone()))?;
                return Ok(());
            }
        }
        bail!(
            "adding to {name} on {} was refused {ADD_TRIES} times",
            self.remote()
        )
    }

    /// Read `branch`'s state from the remote, keeping it for a write.
    fn read_remote(&self, branch: &str) -> Result<Read> {
        let name = state_ref(branch)?;
        // 80: a remote that cannot be reached leaves the state as it was
        // last fetched, readable, and the read says so.
        let fetched = match self.fetch(&name) {
            Ok(f) => {
                self.stale.borrow_mut().remove(branch);
                f
            }
            Err(e) => {
                let last = self.pushed.read_ref(&self.fetched(&name))?;
                let Some(last) = last else { return Err(e) };
                tracing::warn!(
                    sluice.branch = branch,
                    "reading {name} from {} failed, so the state last fetched is read: {e:#}",
                    self.remote()
                );
                let refused = read_refused(&e).cloned().unwrap_or_else(|| ReadRefused {
                    remote: self.remote().into(),
                    asked: name.clone(),
                    said: format!("{e:#}"),
                    note: None,
                });
                self.stale.borrow_mut().insert(branch.into(), refused);
                Some(last.0)
            }
        };
        self.state_at(branch, &name, fetched)
    }

    /// 129: `branch`'s state as last fetched, with no fetch: read whole only
    /// once the fetched commit moved since it was last read.
    fn read_fetched(&self, branch: &str) -> Result<Read> {
        let name = state_ref(branch)?;
        let at = self.pushed.read_ref(&self.fetched(&name))?.map(|r| r.0);
        self.state_at(branch, &name, at)
    }

    /// The statements under a signer's ref, verified, as result lines;
    /// one that does not verify is left out and logged.
    fn signed(&self, signer: &str, key: &SignerKey) -> Result<Vec<ResultLine>> {
        let name = format!("refs/sluice/runs/{signer}");
        let Some(commit) = self.fetch(&name)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (path, (oid, bytes)) in self.tree(&commit)? {
            let seen = (oid, signer.to_string(), key.digest().to_string());
            if let Some(known) = self.verified.borrow().get(&seen) {
                out.extend(known.clone().unwrap_or_default());
                continue;
            }
            let lines = match verify(signer, key, &bytes) {
                Ok(lines) => Some(lines),
                Err(e) => {
                    tracing::warn!(
                        sluice.signer = signer,
                        sluice.path = %path,
                        "a run record that does not verify is left out: {e:#}"
                    );
                    None
                }
            };
            out.extend(lines.clone().unwrap_or_default());
            self.verified.borrow_mut().insert(seen, lines);
        }
        Ok(out)
    }

    /// The branches whose state is on the remote.
    fn remote_branches(&self) -> Result<Vec<String>> {
        let out = self
            .pushed
            .remote_git(&["ls-remote", "--refs"], &["refs/sluice/state/*"])?;
        if !out.status.success() {
            bail!(
                "listing {}'s branches failed: {}",
                self.remote(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split('\t').nth(1))
            .filter_map(|r| r.strip_prefix("refs/sluice/state/"))
            .map(str::to_string)
            .collect())
    }
}

impl RemoteRefs {
    /// Where this host keeps what it last saw of `host`'s beats (77).
    fn beat_observation(&self, host: &str) -> PathBuf {
        self.pushed
            .git_dir()
            .join("sluice-remote")
            .join(self.remote())
            .join("beats")
            .join(format!("{}.json", host.replace('/', "%2F")))
    }

    /// Where this host keeps what it last saw of `branch` (73).
    fn observation(&self, branch: &str) -> Result<PathBuf> {
        self.own_file("observed", branch)
    }

    /// Where this host keeps a refusal of its reads of `branch` (133).
    fn refusal_file(&self, branch: &str) -> Result<PathBuf> {
        self.own_file("refused", branch)
    }

    /// `branch`'s file under `what` in what this host keeps of the remote.
    fn own_file(&self, what: &str, branch: &str) -> Result<PathBuf> {
        state_ref(branch)?;
        Ok(self
            .pushed
            .git_dir()
            .join("sluice-remote")
            .join(self.remote())
            .join(what)
            .join(format!("{branch}.json")))
    }
}

/// A branch's state as read, as the records seam gives it: its version,
/// and its files but the version and anything dotted.
fn snapshot(r: Read) -> Snapshot {
    Snapshot {
        version: r.version,
        files: r
            .files
            .into_iter()
            .filter(|(p, _)| p != VERSION && !p.starts_with('.'))
            .map(|(p, (_, b))| (p, b))
            .collect(),
    }
}

/// 129: a tick's one ask of `pushed`'s remote, under the retry policy
/// (rule core.29): the records of every branch on it and every host's sign
/// of life, in one fetch, each replacing what was last fetched; a branch's
/// records or a host's sign of life gone from the remote is gone here.
/// The same fetch brings every branch on the remote into its tracking
/// ref, the work submitted to it and the combinations formed on its
/// branches, so a reader's read holds all a forecast forms from and asks
/// the remote nothing more for it (64); a branch gone from the remote is
/// gone from its tracking refs, so no revision of it is named as the
/// remote's (124). A remote that cannot be read is a [`ReadRefused`]
/// naming what was asked (133).
pub(crate) fn ask(pushed: &PushedRefs) -> Result<()> {
    let remote = pushed.remote();
    let state = format!("+refs/sluice/state/*:refs/sluice-fetched/{remote}/state/*");
    let hosts = format!("+refs/sluice/hosts/*:refs/sluice-fetched/{remote}/hosts/*");
    let branches = format!("+refs/heads/*:refs/remotes/{remote}/*");
    let heads = format!("+{HEADS}:refs/sluice-fetched/{remote}/heads/*");
    let combinations =
        format!("+refs/sluice/combinations/*:refs/sluice-fetched/{remote}/combinations/*");
    // Design D17: each signer's runs, which the host process's results
    // index reads as this ask last fetched them.
    let runs = format!("+refs/sluice/runs/*:refs/sluice-fetched/{remote}/runs/*");
    let specs = [&state, &hosts, &branches, &heads, &combinations, &runs].map(String::as_str);
    pushed.fetch_specs("refs/sluice/*", &specs, &["--prune"])?;
    Ok(())
}

/// 129: the branches whose state this host fetched from `pushed`'s remote,
/// with the commit each was fetched at, read with no process.
pub(crate) fn fetched_branches(pushed: &PushedRefs) -> Result<Vec<(String, Rev)>> {
    pushed
        .local()
        .refs_under(&format!("refs/sluice-fetched/{}/state/", pushed.remote()))
}
/// A signer's runs on a remote as this host last fetched them, read for a
/// branch whose policy lists `key` for the signer (design D17; rule
/// core.22).
pub struct FetchedRuns<'a> {
    refs: &'a RemoteRefs,
    signer: &'a str,
    key: &'a SignerKey,
}

impl crate::host_process::results::SignerRef for FetchedRuns<'_> {
    fn signer(&self) -> &str {
        self.signer
    }

    fn digest(&self) -> &str {
        self.key.digest()
    }

    fn entries(&self) -> Result<Vec<(String, String)>> {
        let name = format!("refs/sluice/runs/{}", self.signer);
        match self.refs.pushed.read_ref(&self.refs.fetched(&name))? {
            Some(commit) => self.refs.pushed.local().tree_entries(commit.as_str()),
            None => Ok(Vec::new()),
        }
    }

    fn blob(&self, oid: &str) -> Result<Vec<u8>> {
        self.refs.pushed.local().blob(oid)
    }

    fn verify(&self, bytes: &[u8]) -> Result<Vec<ResultLine>> {
        verify(self.signer, self.key, bytes)
    }
}

/// Keep `doc` at `path`, written whole, or remove it with `None` (133).
fn keep_file(path: &Path, doc: Option<&[u8]>) -> Result<()> {
    let Some(doc) = doc else {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        };
    };
    std::fs::create_dir_all(path.parent().expect("a parent"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, doc)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `refs/sluice/state/<branch>`, for a branch git may name.
fn state_ref(branch: &str) -> Result<String> {
    if branch.is_empty()
        || branch
            .split('/')
            .any(|c| c.is_empty() || c.starts_with('.'))
    {
        bail!("{branch:?} cannot name a branch's records");
    }
    Ok(format!("refs/sluice/state/{branch}"))
}

fn tempfile_path(git_dir: &Path, what: &str) -> Result<PathBuf> {
    let dir = git_dir.join("sluice-remote").join(".tmp");
    std::fs::create_dir_all(&dir)?;
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(dir.join(format!("{what}-{}-{n}", std::process::id())))
}

/// A run record's lines, when `key` signed it and its statement is a
/// run's (78, 79). The claims are kept as signed: whether they count is
/// the gate's, by what the policy lets the signer claim. What the executor
/// held the run to and its grant are read from the statement's `held`
/// alone, and a statement with none reads with no limits and no grant
/// (19, 138; design D3).
fn verify(signer: &str, key: &SignerKey, bytes: &[u8]) -> Result<Vec<ResultLine>> {
    let envelope: Envelope = serde_json::from_slice(bytes).context("not a DSSE envelope")?;
    let who = match key {
        SignerKey::Ssh { allowed, .. } => ssh::verify(&envelope, allowed)?
            .map(|a| a.principal.clone())
            .ok_or_else(|| anyhow!("no key {signer} lists signed it"))?,
        SignerKey::Keyless => bail!("{signer}'s keyless signatures are not verified on this host"),
    };
    let s = Statement::from_envelope(&envelope)?;
    let run = s.predicate;
    let held = run.held.clone().unwrap_or_default();
    Ok(run
        .results
        .iter()
        .map(|r| ResultLine {
            schema: RESULT_SCHEMA.into(),
            at: run.at.clone(),
            kind: if r.replayed {
                ResultKind::Replayed
            } else {
                ResultKind::Ran
            },
            task: r.check.clone(),
            hash: r.key.clone(),
            outcome: r.outcome,
            host: Some(who.clone()),
            executor: Some(Enforced {
                kind: run.claims.executor.clone(),
                confined: run.claims.confined,
                processors: None,
                memory: held.memory,
                network: held.network,
                docker: held.docker,
                grant: held.grant.clone(),
            }),
            branch: run.branch.clone(),
            combination: run.combination.clone(),
            worktree: None,
            // A run on no named system claims none (85).
            system: Some(run.claims.system.clone()).filter(|s| !s.is_empty()),
            signer: Some(signer.into()),
            took_ms: r.took_ms,
            attempts: r.attempts,
        })
        .collect())
}

/// What one statement's lines share: their branch, combination, executor
/// kind, confinement, system, and what the executor held them to with its
/// grant (design D3).
type RunOf = (String, String, String, bool, String, Option<HeldKey>);

/// A [`Held`] as a key lines are grouped by.
type HeldKey = (Option<u64>, Option<String>, bool, Option<String>);

/// The statements of `lines`, one per run: the lines of one combination
/// on one system by one executor, held to one set of limits under one
/// grant (19, 138; design D3).
fn statements(lines: &[ResultLine]) -> Vec<Statement> {
    let mut by: BTreeMap<RunOf, Vec<&ResultLine>> = BTreeMap::new();
    for l in lines {
        let e = l.executor.as_ref();
        by.entry((
            l.branch.clone(),
            l.combination.0.clone(),
            e.map(|e| e.kind.clone()).unwrap_or_default(),
            e.is_some_and(|e| e.confined),
            l.system.clone().unwrap_or_default(),
            e.and_then(held_key),
        ))
        .or_default()
        .push(l);
    }
    by.into_iter()
        .map(
            |((branch, combination, executor, confined, system, _), ls)| {
                Statement::of(Run {
                    results: ls
                        .iter()
                        .map(|l| RunResult {
                            check: l.task.clone(),
                            key: l.hash.clone(),
                            outcome: l.outcome,
                            replayed: l.kind == ResultKind::Replayed,
                            took_ms: l.took_ms,
                            attempts: l.attempts,
                        })
                        .collect(),
                    claims: Claims {
                        executor,
                        confined,
                        system,
                    },
                    branch,
                    combination: Rev::new(combination),
                    claim: None,
                    at: ls.iter().map(|l| l.at.clone()).max().unwrap_or_default(),
                    // One held key, so the first line holds it for all.
                    held: ls[0].executor.as_ref().and_then(held_of),
                })
            },
        )
        .collect()
}

/// What `e` held a run to beyond the claims, `None` where it held nothing
/// and named no grant (design D3).
fn held_of(e: &Enforced) -> Option<Held> {
    let held = Held {
        memory: e.memory,
        network: e.network,
        docker: e.docker,
        grant: e.grant.clone(),
    };
    (held != Held::default()).then_some(held)
}

fn held_key(e: &Enforced) -> Option<HeldKey> {
    held_of(e).map(|h| {
        (
            h.memory,
            h.network.map(|n| format!("{n:?}")),
            h.docker,
            h.grant,
        )
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl State for RemoteRefs {
    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
        self.locks.lock(branch)
    }

    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>> {
        self.locks.wait_lock(branch)
    }

    fn publish_head(&self, _branch: &str, head: &Rev) -> Result<()> {
        let name = format!("refs/sluice/heads/{head}");
        let spec = format!("{head}:{name}");
        if let Some(why) = self.push(None, &spec, &name, Some(head.as_str()))?.why() {
            bail!("pushing the head {head} to {} failed: {why}", self.remote());
        }
        Ok(())
    }

    /// Only a head the last fetch of heads found on the remote is removed,
    /// so a round removes each once.
    fn unpublish_head(&self, _branch: &str, head: &Rev) -> Result<()> {
        let name = format!("refs/sluice/heads/{head}");
        let fetched = self.fetched(&name);
        if self.pushed.read_ref(&fetched)?.is_none() {
            return Ok(());
        }
        self.delete(&name)?;
        self.git_ok(&["update-ref", "-d", &fetched])?;
        Ok(())
    }

    fn fetch_heads(&self, _branch: &str) -> Result<()> {
        self.pushed
            .fetch_spec(HEADS, &self.heads_spec(), &["--prune"])?;
        Ok(())
    }

    /// 77: `refs/sluice/hosts/<host>`, rewritten by that host alone.
    fn beat(&self, host: &str, beat: &[u8]) -> Result<()> {
        let blob = self.blob(beat)?;
        let commit = self.commit(
            &BTreeMap::from([("beat.json".to_string(), blob)]),
            None,
            &format!("{host} is alive"),
        )?;
        let name = format!("refs/sluice/hosts/{host}");
        let spec = format!("+{commit}:{name}");
        if let Some(why) = self.push(None, &spec, &name, Some(&commit))?.why() {
            bail!("showing {host} alive on {} failed: {why}", self.remote());
        }
        Ok(())
    }

    /// Every host's last beat: its commit, which changes with every beat,
    /// and its document.
    ///
    /// 129: once a tick's ask fetched them, the next answer reads what it
    /// fetched, with no fetch of its own.
    fn beats(&self) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        if !self.asked.replace(false) && !self.held.get() {
            let spec = format!(
                "+refs/sluice/hosts/*:refs/sluice-fetched/{}/hosts/*",
                self.remote()
            );
            let out = self.pushed.remote_git(
                &[
                    "fetch",
                    "--quiet",
                    "--no-tags",
                    "--no-write-fetch-head",
                    "--prune",
                ],
                &[&spec],
            )?;
            if !out.status.success() {
                bail!(
                    "reading hosts' signs of life from {} failed: {}",
                    self.remote(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
        let prefix = format!("refs/sluice-fetched/{}/hosts/", self.remote());
        let mut out = BTreeMap::new();
        for (host, at) in self.pushed.local().refs_under(&prefix)? {
            let beat = self
                .tree(at.as_str())?
                .remove("beat.json")
                .map(|(_, b)| b)
                .unwrap_or_default();
            out.insert(host, (at.0, beat));
        }
        Ok(out)
    }

    fn observed_beat(&self, host: &str) -> Result<Option<(String, String)>> {
        let path = self.beat_observation(host);
        match std::fs::read(&path) {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_slice(&b)?;
                Ok(v["marker"]
                    .as_str()
                    .map(str::to_string)
                    .zip(v["at"].as_str().map(str::to_string)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn observe_beat(&self, host: &str, marker: &str, at: &str) -> Result<()> {
        let path = self.beat_observation(host);
        std::fs::create_dir_all(path.parent().expect("a parent"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::json!({"marker": marker, "at": at}).to_string(),
        )?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn publish_candidate(&self, branch: &str, id: &str, rev: &Rev) -> Result<()> {
        state_ref(branch)?;
        let name = format!("refs/sluice/combinations/{branch}/{id}");
        let spec = format!("+{rev}:{name}");
        if let Some(why) = self.push(None, &spec, &name, Some(rev.as_str()))?.why() {
            bail!(
                "opening the combination {id} on {} failed: {why}",
                self.remote()
            );
        }
        Ok(())
    }

    fn unpublish_candidate(&self, branch: &str, id: &str) -> Result<()> {
        self.delete_if_present(&format!("refs/sluice/combinations/{branch}/{id}"))
    }

    fn fetch_candidates(&self, branch: &str) -> Result<()> {
        state_ref(branch)?;
        let asked = format!("refs/sluice/combinations/{branch}/*");
        self.pushed
            .fetch_spec(&asked, &self.combinations_spec(branch), &["--prune"])?;
        Ok(())
    }

    /// Into the ref the branch's fetch of every candidate fills, so the
    /// two agree; a candidate the remote no longer holds fetches nothing.
    fn fetch_candidate(&self, branch: &str, id: &str) -> Result<()> {
        state_ref(branch)?;
        self.fetch(&format!("refs/sluice/combinations/{branch}/{id}"))?;
        Ok(())
    }

    fn observed(&self, branch: &str) -> Result<Option<(u64, String)>> {
        let path = self.observation(branch)?;
        match std::fs::read(&path) {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_slice(&b)?;
                Ok(v["version"]
                    .as_u64()
                    .zip(v["at"].as_str().map(str::to_string)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn observe(&self, branch: &str, version: u64, at: &str) -> Result<()> {
        let path = self.observation(branch)?;
        std::fs::create_dir_all(path.parent().expect("a parent"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::json!({"version": version, "at": at}).to_string(),
        )?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn stale(&self, branch: &str) -> Option<ReadRefused> {
        self.stale.borrow().get(branch).cloned()
    }

    fn intake_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.own_file("intake", branch)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn keep_intake_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        keep_file(&self.own_file("intake", branch)?, doc)
    }

    fn refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.refusal_file(branch)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn keep_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        let path = self.refusal_file(branch)?;
        let Some(doc) = doc else {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            };
        };
        std::fs::create_dir_all(path.parent().expect("a parent"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, doc)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn read(&self, branch: &str) -> Result<Snapshot> {
        Ok(snapshot(self.read_remote(branch)?))
    }

    /// 129: the tick's one ask of the remote, a refusal read as every
    /// branch on it stale.
    fn refresh(&self) -> Result<()> {
        let refused = match ask(&self.pushed) {
            Ok(()) => None,
            Err(e) => Some(read_refused(&e).cloned().ok_or(e)?),
        };
        self.asked(refused.as_ref())
    }

    fn read_held(&self, branch: &str) -> Result<Snapshot> {
        Ok(snapshot(self.read_fetched(branch)?))
    }

    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.read_all_finished(branch)?.remove(name))
    }

    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        let r = self.read_remote(branch)?;
        let prefix = format!("{FINISHED}/");
        Ok(r.files
            .into_iter()
            .filter_map(|(p, (_, b))| p.strip_prefix(&prefix).map(|n| (n.to_string(), b)))
            .collect())
    }

    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written> {
        let held = self.read.borrow().get(branch).cloned();
        let read = match held {
            Some(r) if r.version == read_at => r,
            _ => {
                let r = self.read_remote(branch)?;
                if r.version != read_at {
                    return Ok(Written::Refused);
                }
                r
            }
        };
        let mut files: BTreeMap<String, String> = read
            .files
            .iter()
            .map(|(p, (oid, _))| (p.clone(), oid.clone()))
            .collect();
        for (path, bytes) in &changes.put {
            files.insert(path.clone(), self.blob(bytes)?);
        }
        for (path, bytes) in &changes.append {
            let mut all = read
                .files
                .get(path)
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            all.extend_from_slice(bytes);
            files.insert(path.clone(), self.blob(&all)?);
        }
        for path in &changes.remove {
            files.remove(path);
        }
        let version = read_at + 1;
        files.insert(
            VERSION.into(),
            self.blob(format!("{version}\n").as_bytes())?,
        );
        let commit = self.commit(&files, None, &format!("{branch} at {version}"))?;
        let name = state_ref(branch)?;
        if !self.push_leased(&commit, &name, read.commit.as_deref())? {
            self.read.borrow_mut().remove(branch);
            return Ok(Written::Refused);
        }
        self.read.borrow_mut().remove(branch);
        // 82: a submission moved to the finished records has left sluice,
        // and its head with it.
        let prefix = format!("{FINISHED}/");
        for (path, bytes) in &changes.put {
            if !path.starts_with(&prefix) || !path.ends_with(".json") {
                continue;
            }
            let head = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|v| v["head"].as_str().map(str::to_string));
            if let Some(head) = head {
                self.delete_if_present(&format!("refs/sluice/heads/{head}"))?;
            }
        }
        Ok(Written::Written(version))
    }

    fn branches(&self) -> Result<Vec<String>> {
        self.remote_branches()
    }

    /// The remote's count, one for every branch whose records it holds,
    /// across every host (record 0114).
    fn allocate_id(&self, _branch: &str) -> Result<u64> {
        let name = "refs/sluice/ids";
        for _ in 0..ADD_TRIES {
            let tip = self.fetch(name)?;
            let next: u64 = match &tip {
                Some(c) => match self.tree(c)?.get("next") {
                    Some((_, b)) => std::str::from_utf8(b)?.trim().parse()?,
                    None => 1,
                },
                None => 1,
            };
            let files = BTreeMap::from([(
                "next".to_string(),
                self.blob(format!("{}\n", next + 1).as_bytes())?,
            )]);
            let commit = self.commit(&files, None, &format!("next id {}", next + 1))?;
            if self.push_leased(&commit, name, tip.as_deref())? {
                return Ok(next);
            }
        }
        bail!(
            "taking a submission id on {} was refused {ADD_TRIES} times",
            self.remote()
        )
    }

    /// This host's own replays alone: the profile keeps no signer's key,
    /// so a branch's signed runs are read with
    /// [`RemoteRefs::results_with`] and its own policy's list (78).
    fn results_for(&self, _: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        Ok(ask.answer(&self.results_with(&BTreeMap::new())?))
    }

    /// Nothing this host signs counts: with no list, no signed run is
    /// verified here (78, 87).
    fn verifies_own(&self, _: &str, _: &Rev) -> Result<bool> {
        Ok(false)
    }

    /// Could-not-run-1 116: every result set aside on the remote, one file
    /// each under `refs/sluice/set-aside`. A set-aside only stops a result
    /// counting, so any host that submits may add one. 129 (design D16):
    /// held, as this host last fetched them, with no fetch.
    fn set_asides(&self) -> Result<Vec<u8>> {
        let commit = match self.held.get() {
            true => self.pushed.read_ref(&self.fetched(SET_ASIDE))?.map(|r| r.0),
            false => self.fetch(SET_ASIDE)?,
        };
        let Some(commit) = commit else {
            return Ok(Vec::new());
        };
        let mut bytes = Vec::new();
        for (_, (_, b)) in self.tree(&commit)? {
            bytes.extend(&b);
            if !b.ends_with(b"\n") {
                bytes.push(b'\n');
            }
        }
        Ok(bytes)
    }

    fn set_aside(&self, line: &[u8]) -> Result<()> {
        let name = format!("{}.json", sha256_hex(line));
        self.add(
            SET_ASIDE,
            &BTreeMap::from([(name, line.to_vec())]),
            "a result set aside",
        )
    }

    /// A run is signed by this host's key and added to its signer's ref;
    /// a replay is kept here alone. With no signing key nothing is
    /// recorded, and the round says so (78).
    fn record_results(&self, lines: &[u8]) -> Result<()> {
        let lines: Vec<ResultLine> = parse_lines(lines)?;
        // A replay is signed with the run it came in, as a replay: the
        // signer saw it answered from the cache, and it counts only
        // through a pass signed for its key (108; rule core.9).
        let executor = lines.iter().find_map(|l| l.executor.clone());
        let ran: Vec<ResultLine> = lines
            .into_iter()
            .map(|mut l| {
                if l.executor.is_none() {
                    l.executor = executor.clone();
                }
                l
            })
            .collect();
        if ran.is_empty() {
            return Ok(());
        }
        let Some(signing) = &self.signing else {
            bail!(
                "this host has no signing key, so it records no result on {}: \
                 name one under [signing] in the host's config",
                self.remote()
            );
        };
        let mut files = BTreeMap::new();
        for s in statements(&ran) {
            let mut e = s.envelope()?;
            ssh::sign(&mut e, &signing.key)?;
            let bytes = serde_json::to_vec(&e)?;
            let path = format!(
                "{}/{}/{}.json",
                s.predicate.combination.0,
                if s.predicate.claims.system.is_empty() {
                    "any"
                } else {
                    &s.predicate.claims.system
                },
                &sha256_hex(&bytes)[..16]
            );
            files.insert(path, bytes);
        }
        self.add(
            &format!("refs/sluice/runs/{}", signing.signer),
            &files,
            "runs",
        )
    }

    /// The outputs are kept here as `local files` keeps them, and on the
    /// remote for every other host; only the last `KEPT_LOGS` candidates
    /// of the branch keep theirs there.
    fn keep_logs(&self, branch: &str, candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>> {
        let kept = self.own.keep_logs(branch, candidate, logs)?;
        let files: BTreeMap<String, Vec<u8>> = logs
            .iter()
            .map(|l| {
                (
                    format!("{}.log", l.task.replace('/', "_")),
                    l.text.clone().into_bytes(),
                )
            })
            .collect();
        let name = format!("refs/sluice/logs/{branch}/{candidate}");
        let mut blobs = BTreeMap::new();
        for (p, b) in &files {
            blobs.insert(p.clone(), self.blob(b)?);
        }
        let commit = self.commit(&blobs, None, &format!("logs of {candidate}"))?;
        let spec = format!("+{commit}:{name}");
        if let Some(why) = self.push(None, &spec, &name, Some(&commit))?.why() {
            bail!(
                "keeping {candidate}'s logs on {} failed: {why}",
                self.remote()
            );
        }
        let listed = self.pushed.remote_git(
            &["ls-remote", "--refs"],
            &[&format!("refs/sluice/logs/{branch}/*")],
        )?;
        let mut names: Vec<String> = String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
            .collect();
        // Candidates are numbered in order of forming: the oldest go.
        names.sort_by_key(|n| {
            n.rsplit('/')
                .next()
                .and_then(|c| {
                    c.trim_start_matches('c')
                        .split('-')
                        .next()?
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(0)
        });
        let excess = names.len().saturating_sub(KEPT_LOGS);
        let gone: BTreeSet<&String> = names.iter().take(excess).collect();
        for n in gone {
            self.delete(n)?;
        }
        Ok(kept)
    }

    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
        self.own.progress(branch)
    }

    fn put_progress(
        &self,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        self.own
            .put_progress_under(&self.locks, branch, candidate, tasks)
    }
}
