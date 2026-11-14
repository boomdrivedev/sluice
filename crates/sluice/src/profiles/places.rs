//! The place seam (B.1): a checkout of exactly the candidate, and its
//! removal with everything started in it ended (C.1; 25, 32 to 35; rules
//! core.10, core.16; `docs/plans/phase-1-integrations.md` section 5).
//!
//! The one method, a git worktree (ADR 0013): `git worktree add
//! --detach` under `<common>/sluice/.places/`, with no hook run, since
//! nothing of the candidate's may run outside the executor. `[checkout]`
//! names no method; an older revision's `method = "git-worktree"` is read
//! as before (core.15), and any other method is refused.
//!
//! A place holds its candidate and nothing restored from another place:
//! the caches its runs share are the host's, which the check seam gives
//! them (35; ADR 0049). A policy that still declares `[[checkout.cache]]`
//! is refused when it is read, so none reaches a place (ADR 0049), and
//! the caches an earlier sluice saved under `<common>/sluice/.caches/`
//! are removed when a place is made.
//!
//! A place holds its ports themselves when `[checkout] ports` names any
//! (31, 32; record 0016 as amended 2026-11-13; rule core.10): the host
//! holds each port a live place holds, by the port and across every
//! repository, under `$XDG_STATE_HOME/sluice/ports/held/<port>` (else
//! `~/.local/state`), each naming the record of the place that holds it,
//! under `ports/.lock`; a port whose record is gone is free again. A new
//! place takes, from `ports_from`, 20000 by default, above worktrunk's
//! `hash_port` range, the lowest run of as many ports as its policy names
//! in which no port is held by a live place and each port is free, one
//! port per name in the order written. Its record is written first, so a
//! hold names a record that exists, then each port is held, then the
//! record is written with `ports`, each name and its port, which every
//! later reading takes and none reckons again from a policy; a release
//! frees each port its record names. An older sluice's blocks,
//! `ports/<n>`, are read by nothing. The place gives its checks
//! `SLUICE_PLACE`, a short name unique to it, and `SLUICE_PORT_<NAME>`
//! for each name.
//!
//! A session's own worktree is adopted, never made (session-runs-1
//! clauses 90, 94, 95): its record, `kind: "session"`, names the worktree
//! and its branch and holds its ports from its first run
//! until `sluice release`, which ends its runs, runs the release command
//! once, removes its scratch directory and lets its ports go, and never
//! removes the worktree. Its record keeps when its last run started, and
//! `session_places` lists every such record with whether its worktree
//! still exists, so the core can release one whose worktree is gone:
//! its stored release run once from its scratch directory, then freed
//! (clause 97; 123).
//!
//! A release of a session's place or of a preview that fails keeps the
//! place's record, with what failed, how many times it was tried and
//! whether it is held (123): a session's place at once, since no round
//! releases it, and a preview once it has been tried more than its
//! branch's `release_retries`. [`Checkout::free`] lets go of a place
//! with nothing run in its name. A place whose working copy is gone has
//! the release its last run stored run once, in its scratch directory,
//! with the ports and environment stored with it, and is then freed
//! whatever it did ([`Checkout::release_gone`]); its record notes the
//! release started first, so a free that fails part way and is tried
//! again runs nothing.
//!
//! A candidate's or a preview's place is gone too where git no longer
//! lists its folder as a worktree of the repository, as git leaves a
//! folder whose removal it stopped part way; git's list is read from the
//! repository's files, the folder matched by what it is on disk and not
//! by how its path is spelled, with no process started. sluice removes
//! what is left of such a folder itself, since git refuses to; a folder
//! git lists goes by `git worktree remove`. A session's place is gone
//! only once its worktree is no directory (record 0115; rules core.16,
//! core.20).
//!
//! A preview (previews-1 clauses 98 to 101) is a place made as a
//! candidate's is, at a revision its branch has landed, and released only
//! when asked, or by the core when its command failed; one whose checkout
//! is gone is freed. Its record, `kind: "preview"`, is kept apart from the others, at
//! `<common>/sluice/.previews/<name>.json` beside its checkout, so a
//! sluice that predates previews never reads one. Each port it holds is
//! given a name too, `SLUICE_NAME_<NAME>`: one DNS label made of the
//! port's name, the revision, the branch and the repository, unique among
//! the repository's previews (99). A preview released because its
//! command failed keeps its record, holding nothing, with what the command
//! printed, until it is released again.
//!
//! However a preview is let go, released or freed, the containers the
//! host's engine holds for it end between its release command, where one
//! runs, and its checkout's removal, while it still holds its ports and
//! names (34; previews-1 clause 101; record 0121; rule core.10): those
//! that mount its checkout or scratch, publish one of its ports, or carry
//! its `SLUICE_PLACE` as their `sluice.place` label. The end is the check
//! seam's, a [`ContainerEnd`] the binding hands this profile. A container
//! it cannot remove keeps the preview whole: a release keeps the failure
//! as a failing release command's, and a free fails before it lets
//! anything go, so a gone preview is freed only once its containers end,
//! its stored release never run again.
//!
//! What made each place is recorded before the place is made, at
//! `<common>/sluice/.places/<name>.json`, so `release` needs no policy and
//! touches only what that record names: every path and branch it touches
//! is derived from the place's name or read from its record, never
//! searched for (rule core.16).
//!
//! A place's scratch directory, its checks' `SLUICE_TMP`, is under the
//! host user's own directory, `/tmp` resolved and `sluice-<uid>`, apart
//! from every repository: 10 hex digits of the SHA-1 of the repository's
//! common directory, then 10 of the place's name, the same length for
//! every place of every repository on the host whatever its name, its
//! worktree's location or the repository's (35 as amended). The user's
//! directory and the repository's under it are made readable, writable
//! and searchable by the user alone, and one that is a link, another
//! user's or open to others is refused and left as it is. A place's record
//! names its scratch before it is made, and every run, release and gone
//! release reads it from there, making it again where the system emptied
//! it (rule core.30). A candidate's or a preview's place made before keeps
//! its scratch where its record names, under the repository's
//! `<common>/sluice/.tmp/`, or, on a record an older sluice wrote, which
//! names none, at `.tmp/` and the place's name as one directory name,
//! until the place is released. A session's place so made takes the short
//! path at its first run with none of its runs going, its former scratch
//! moved there (record 0063 as amended 2026-11-05).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::{
    Adopted, CheckedOut, Checkout, GoneRelease, LastResult, Limits, Place, PreviewPlace,
    ReleaseFailure, ReleasePolicy, Released, Rev, RunHandle, SessionPlace, Setting, Settings,
};

use crate::runs::{dir_name, flat_name, ReleaseWith, Request, Runs, SESSION};

/// Builds a session's place's release command from the policy's `[tests]`
/// and limits, under the executor its runs had, writing what the
/// toolchain and the executor need in the directory given: the check
/// seam's, handed to this profile by the binding (rule core.2).
pub type ReleaseBuilder = Box<dyn Fn(&Place, &Settings, &Limits, &Path) -> Result<Option<Request>>>;

/// Ends what the engine holds for a place: the check seam's, handed to
/// this profile by the binding (rule core.2; record 0121).
pub type ContainerEnd = Box<dyn Fn(&Place) -> Result<()>>;

/// Whose a place is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    /// A candidate's, made by sluice and removed on release (25).
    #[default]
    Made,
    /// A session's own worktree, adopted for its runs and never removed.
    Session,
    /// A preview, made at a landed revision and released when asked.
    Preview,
}

/// What made a place: `<common>/sluice/.places/<name>.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Made {
    #[serde(default)]
    kind: Kind,
    method: String,
    /// A session's place: the branch whose policy its runs are held to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    /// The ports it holds, `SLUICE_PORT_<NAME>` and its port, given when
    /// it was made, or a session's at its first run, and held until its
    /// release (31, 90; record 0016 as amended 2026-11-13).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    ports: BTreeMap<String, String>,
    /// The checkout, once it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    /// The checkout cache an older sluice kept on the record: read, and
    /// ignored; a policy that still declares one is refused before any
    /// place is made (35; ADR 0049).
    #[serde(rename = "caches", default, skip_serializing)]
    _caches: serde::de::IgnoredAny,
    /// A session's place: when its last run started (session-runs-1
    /// clause 97); a preview: when it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_run: Option<String>,
    /// A preview's revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision: Option<Rev>,
    /// A preview's `SLUICE_NAME_<NAME>` for each port (99).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    names: BTreeMap<String, String>,
    /// A preview's addresses, filled in (99).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    addresses: BTreeMap<String, String>,
    /// A preview's command's run, once started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    /// A preview released because its command failed: what it printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failed: Option<String>,
    /// A session's place: how its last run ended (session-runs-1 clause
    /// 97).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_result: Option<LastResult>,
    /// A session's place or a preview: its release failed, and how (123).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    release_failed: Option<ReleaseFailure>,
    /// Its working copy was gone, and the release its runs stored was
    /// started then: never started again (123).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gone_release: Option<String>,
    /// Its scratch directory, named before it is made, under the user's own
    /// directory at a path that grows with neither the place's name nor
    /// where its checkout or the repository is (35 as amended, rule
    /// core.30); under the repository's `sluice/.tmp/` for a place made
    /// before; none on a record an older sluice wrote, whose scratch is
    /// where that sluice put it. A session's place's former scratch is
    /// replaced at its next run with none going ([`Places::adopt`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scratch: Option<PathBuf>,
}

impl Made {
    fn new(kind: Kind, method: &str) -> Made {
        Made {
            kind,
            method: method.into(),
            branch: None,
            ports: BTreeMap::new(),
            path: None,
            _caches: serde::de::IgnoredAny,
            last_run: None,
            revision: None,
            names: BTreeMap::new(),
            addresses: BTreeMap::new(),
            run: None,
            failed: None,
            last_result: None,
            release_failed: None,
            gone_release: None,
            scratch: None,
        }
    }
}

/// 35, rule core.30: the name of the scratch directory of the place
/// `name`, the first 10 hex digits of the SHA-1 of its name, so its path
/// is the same length for every place of the repository. Never a former
/// scratch's name: every place's name holds a `/`, which a `dir_name`
/// writes `+2F`, or is shortened with `+~`.
pub fn scratch_name(name: &str) -> String {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(name.as_bytes());
    hasher
        .try_finalize()
        .expect("a place's name is no SHA-1 collision")
        .to_hex_with_len(10)
        .to_string()
}

/// 35 as amended, rule core.30: the host user's own directory, which
/// holds the scratch of every repository's checkouts: `/tmp` resolved,
/// then `sluice-` and the effective user's id, `/private/tmp/sluice-501`
/// on macOS. Resolved, since the sandbox matches resolved paths; made from
/// nothing a process's environment says, so every process of the user on
/// the host finds the same directory.
pub fn user_scratch() -> Result<PathBuf> {
    let tmp = std::fs::canonicalize("/tmp").context("resolving /tmp")?;
    Ok(tmp.join(format!("sluice-{}", nix::unistd::geteuid())))
}

/// The tests' way to name the directory standing for the user's own,
/// [`user_scratch`]: where set, the binding hands it to
/// [`Places::with_scratch_root`], since sluice's gate runs the tests under
/// the sandbox, where a binary a test starts may write the tests' own
/// directories and not `/tmp/sluice-<uid>`. A host sets none.
pub const USER_SCRATCH_VAR: &str = "SLUICE_USER_SCRATCH";

/// 35 as amended, rule core.30: the repository's key, the first 10 hex
/// digits of the SHA-1 of its common directory's path, so two copies of a
/// repository keep their scratch apart, and no process is started for it
/// (129).
pub fn repository_key(common: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(common.as_os_str().as_bytes());
    hasher
        .try_finalize()
        .expect("a path is no SHA-1 collision")
        .to_hex_with_len(10)
        .to_string()
}

/// 35 as amended, rule core.30: why a directory of `kind`, owned by
/// `owner` with `mode`, is not one the user `uid` may keep a scratch
/// under: a link, not a directory, another user's, or one others may
/// enter. sluice changes nothing it finds, a mode the user widened among
/// it.
fn own_dir(
    kind: std::fs::FileType,
    owner: u32,
    mode: u32,
    uid: u32,
) -> std::result::Result<(), &'static str> {
    if kind.is_symlink() {
        Err("is a link")
    } else if !kind.is_dir() {
        Err("is not a directory")
    } else if owner != uid {
        Err("belongs to another user")
    } else if mode & 0o077 != 0 {
        Err("may be entered by others")
    } else {
        Ok(())
    }
}

/// `dir`, read as `meta`, refused with its path unless it is a directory
/// of the user's own, closed to everyone else (35 as amended, rule
/// core.30), in the words both the making and the sweep say it in.
fn is_own(dir: &Path, meta: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    own_dir(
        meta.file_type(),
        meta.uid(),
        meta.mode(),
        nix::unistd::geteuid().as_raw(),
    )
    .map_err(|what| {
        anyhow!(
            "{} is not a directory of this user's own, closed to everyone else: {what}; remove it, and sluice makes it again",
            dir.display()
        )
    })
}

/// `dir` made readable, writable and searchable by the user alone, or,
/// where it is there, refused with its path unless it is so (35 as
/// amended, rule core.30): nothing of one refused is changed or removed.
/// One another process of the user made first is no error.
fn make_own(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
            return Err(e).with_context(|| format!("creating {}", dir.display()));
        }
        _ => {}
    }
    let meta =
        std::fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
    is_own(dir, &meta)
}

/// Every directory in `dir`, pushed onto `out`; none where `dir` is not
/// there.
fn list_dirs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
    };
    for e in entries {
        let e = e?;
        if e.file_type()?.is_dir() {
            out.push(e.path());
        }
    }
    Ok(())
}

/// The repository's directory under the user's, which holds the scratch
/// of the checkouts the repository at the common directory `common` makes
/// now: what a test's repository leaves there is removed through it.
pub fn repository_scratch(common: &Path) -> Result<PathBuf> {
    Ok(user_scratch()?.join(repository_key(common)))
}

/// Whether `dir` is named as [`scratch_name`] names a scratch directory.
fn is_scratch_name(dir: &OsString) -> bool {
    dir.to_str()
        .is_some_and(|d| d.len() == 10 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// What the records under `.places/` and `.previews/` name of the
/// repository's two scratch roots, so [`Places::remove_unnamed`] removes
/// nothing a record may name (35 as amended, rule core.30). Names alone
/// are compared, in either root: a record naming a scratch under the
/// user's directory and one naming the same 10 hex digits under `.tmp/`
/// are one checkout's name made at two times, so keeping both while
/// either record stands keeps nothing another checkout needs, and the
/// former goes once its name is free; whole paths would need every
/// process to spell a record's path alike, which records older sluices
/// wrote do not promise.
#[derive(Default)]
struct Named {
    /// The file names of the scratch directories records name.
    dirs: BTreeSet<OsString>,
    /// A record names no scratch, or cannot be read, and its place's name
    /// cannot be read back, its file name cut short: it may name any
    /// directory of the former form.
    former: bool,
    /// A record cannot be read and its file name was cut short: it may
    /// name any directory of the short form too.
    short: bool,
}

impl Named {
    /// Whether a record may name the scratch directory `dir`.
    fn keeps(&self, dir: &OsString) -> bool {
        self.dirs.contains(dir)
            || if is_scratch_name(dir) {
                self.short
            } else {
                self.former
            }
    }
}

/// The name of a candidate's place, read back from its record's file name
/// `stem`; none for a name cut short, which says nothing of the name it
/// was.
fn read_name(stem: &str) -> Option<String> {
    if stem.contains("%~") {
        return None;
    }
    Some(
        stem.replace("%2F", "/")
            .replace("%2E", ".")
            .replace("%25", "%"),
    )
}

/// The ports `[checkout] ports` names, from `ports_from`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ports {
    names: Vec<String>,
    from: u32,
}

/// Where a place's ports begin by default: above `hash_port`'s 10000 to
/// 19999, below every system's ephemeral range.
const PORTS_FROM: u32 = 20000;

impl Ports {
    fn read(checkout: &Settings) -> Result<Option<Ports>> {
        let names = match checkout.get("ports") {
            None => return Ok(None),
            Some(Setting::List(xs)) if xs.is_empty() => return Ok(None),
            Some(Setting::List(xs)) => xs.clone(),
            Some(_) => bail!("[checkout] ports must be a list of names"),
        };
        let mut seen = std::collections::BTreeSet::new();
        for n in &names {
            if n.is_empty()
                || !n
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                bail!("[checkout] ports: {n:?} must be letters, digits, '-' and '_'");
            }
            if !seen.insert(env_name(n)) {
                bail!("[checkout] ports names {} twice", env_name(n));
            }
        }
        let from = match checkout.get("ports_from") {
            None => PORTS_FROM,
            Some(Setting::Text(t)) => match t.parse::<u32>() {
                Ok(n) if (1024..65536).contains(&n) => n,
                _ => bail!("[checkout] ports_from = {t}: a port from 1024 to 65535"),
            },
            Some(_) => bail!("[checkout] ports_from must be a number"),
        };
        Ok(Some(Ports { names, from }))
    }
}

/// `SLUICE_PORT_<NAME>`: the name upper-cased, `-` written `_`.
fn env_name(name: &str) -> String {
    format!("SLUICE_PORT_{}", env_part(name))
}

/// `SLUICE_NAME_<NAME>`: a preview's name for the port `name` (99).
fn name_env(name: &str) -> String {
    format!("SLUICE_NAME_{}", env_part(name))
}

fn env_part(name: &str) -> String {
    name.to_ascii_uppercase().replace('-', "_")
}

/// The longest a DNS label may be.
const LABEL: usize = 63;

/// `text` as part of a DNS label: lower-case letters, digits and single
/// hyphens, none at either end.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Previews-1 clause 99: a preview's name for its port `port`, one DNS
/// label: `<port>-<rev>--<branch>--<repository>`, the port and the
/// revision kept whole; past 63 characters the rest is cut, and `hash`,
/// six characters of a hash of the whole, added so two long names that
/// share a beginning stay apart.
fn label(port: &str, rev: &str, branch: &str, repository: &str, hash: &str) -> String {
    let head = format!("{}-{rev}--", slug(port));
    let tail = format!("{}--{}", slug(branch), slug(repository));
    let whole = format!("{head}{tail}");
    if whole.len() <= LABEL {
        return whole.trim_end_matches('-').to_string();
    }
    let room = LABEL.saturating_sub(head.len() + 1 + hash.len());
    let cut = tail[..room.min(tail.len())].trim_end_matches('-');
    format!("{head}{cut}-{hash}")
}

/// The repository's name, from its common directory as `devurl` takes
/// it: `<project>.git` names the project, and a `.git`, `.bare` or `git`
/// directory names the one that holds it.
fn repository(common: &Path) -> String {
    let base = common
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = base.strip_suffix(".git").unwrap_or(&base);
    let name = name.trim_start_matches('.');
    match name {
        "" | "bare" | "git" => common
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repository".into()),
        n => n.to_string(),
    }
}

/// Each `{name.<port>}` and `{port.<port>}` of `template` filled in; an
/// error naming the address when it names a port that is not listed.
fn fill(
    label: &str,
    template: &str,
    names: &BTreeMap<String, String>,
    ports: &BTreeMap<String, String>,
) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let close = rest[i..]
            .find('}')
            .ok_or_else(|| anyhow!("[preview] addresses {label} leaves a {{ open"))?;
        let inner = &rest[i + 1..i + close];
        let value = match inner.split_once('.') {
            Some(("name", p)) => names.get(&name_env(p)),
            Some(("port", p)) => ports.get(&env_name(p)),
            _ => None,
        };
        let Some(value) = value else {
            bail!("[preview] addresses {label} names {{{inner}}}, a port [checkout] ports does not list");
        };
        out.push_str(value);
        rest = &rest[i + close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Whether nothing listens on `port` and it can be bound.
fn port_free(port: u16) -> bool {
    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok()
        && TcpStream::connect_timeout(&local, Duration::from_millis(100)).is_err()
}

/// The host's held ports, shared by every repository.
fn ports_dir() -> Result<PathBuf> {
    Ok(crate::instance::state_dir()?.join("ports"))
}

/// The host's ports, locked while held.
fn lock_ports(dir: &Path) -> Result<File> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let lock = File::create(dir.join(".lock"))?;
    lock.lock()?;
    Ok(lock)
}

/// Whether `port`, as `held/<port>` in `held` says, is held by a live
/// place other than the one whose record is `record`: one whose record
/// exists.
fn held_by_another(held: &Path, port: u32, record: &Path) -> Result<bool> {
    match std::fs::read(held.join(port.to_string())) {
        Ok(holder) => {
            let holder = PathBuf::from(String::from_utf8_lossy(&holder).trim());
            Ok(holder != record && holder.exists())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// 31, 32, record 0016 as amended 2026-11-13: hold, for the place whose
/// record is `record`, the lowest run of as many ports as `ports` names,
/// from its `ports_from`, in which no port is held by a live place and
/// each port is free; each name's `SLUICE_PORT_<NAME>` and its port.
fn hold_ports(dir: &Path, ports: &Ports, record: &Path) -> Result<BTreeMap<String, String>> {
    let _lock = lock_ports(dir)?;
    let held = dir.join("held");
    std::fs::create_dir_all(&held).with_context(|| format!("creating {}", held.display()))?;
    let n = ports.names.len() as u32;
    let mut first = ports.from;
    while first + n <= 65536 {
        let mut taken = None;
        for port in first..first + n {
            if held_by_another(&held, port, record)? || !port_free(port as u16) {
                taken = Some(port);
                break;
            }
        }
        if let Some(port) = taken {
            first = port + 1;
            continue;
        }
        let mut out = BTreeMap::new();
        for (name, port) in ports.names.iter().zip(first..) {
            std::fs::write(
                held.join(port.to_string()),
                record.to_string_lossy().as_bytes(),
            )?;
            out.insert(env_name(name), port.to_string());
        }
        return Ok(out);
    }
    bail!(
        "no run of {n} free ports from {} is left on this host",
        ports.from
    )
}

/// Let go of each port of `ports` the place whose record is `record`
/// holds; a port another place holds now is left to it.
fn free_ports(dir: &Path, ports: &BTreeMap<String, String>, record: &Path) -> Result<()> {
    if ports.is_empty() {
        return Ok(());
    }
    let _lock = lock_ports(dir)?;
    for port in ports.values() {
        let path = dir.join("held").join(port);
        match std::fs::read(&path) {
            Ok(holder) if Path::new(String::from_utf8_lossy(&holder).trim()) == record => {
                std::fs::remove_file(&path)?;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub struct Places {
    common: PathBuf,
    runs: Runs,
    /// Where the host's ports are held; the default when `None`.
    ports: Option<PathBuf>,
    /// The directory standing for the user's, [`user_scratch`] when
    /// `None`.
    scratch_root: Option<PathBuf>,
    /// How a session's place's release command is built from its policy.
    release: Option<ReleaseBuilder>,
    /// How a preview's containers are ended (record 0121); none ended when
    /// `None`.
    containers: Option<ContainerEnd>,
    /// The repository at its common directory, as gix reads it, once
    /// opened: its worktrees, read with no process (129).
    repo: std::cell::OnceCell<gix::Repository>,
    /// The repository's key, [`repository_key`], once made.
    key: std::cell::OnceCell<String>,
}

impl Places {
    pub fn new(common: &Path, runs: Runs) -> Self {
        Places {
            common: common.to_path_buf(),
            runs,
            ports: None,
            scratch_root: None,
            release: None,
            containers: None,
            repo: std::cell::OnceCell::new(),
            key: std::cell::OnceCell::new(),
        }
    }

    /// The repository at its common directory, opened at its first
    /// reading.
    fn repo(&self) -> Result<&gix::Repository> {
        if let Some(r) = self.repo.get() {
            return Ok(r);
        }
        let r = gix::open(&self.common)
            .map_err(|e| anyhow!("opening the repository at {}: {e}", self.common.display()))?;
        Ok(self.repo.get_or_init(|| r))
    }

    /// Build a session's place's release command with `builder`.
    pub fn with_release(mut self, builder: ReleaseBuilder) -> Self {
        self.release = Some(builder);
        self
    }

    /// End a preview's containers with `end` whenever it is let go
    /// (record 0121).
    pub fn with_containers(mut self, end: ContainerEnd) -> Self {
        self.containers = Some(end);
        self
    }

    /// 34, previews-1 clause 101, record 0121: end the containers the
    /// preview `name`, as its record `made` holds it, left, by the place
    /// its command was given; nothing where no end was handed this
    /// profile.
    fn end_containers(&self, name: &str, made: &Made) -> Result<()> {
        match &self.containers {
            Some(end) => end(&self.preview_run_place(name, made)?),
            None => Ok(()),
        }
    }

    /// The session's place of `worktree`: its canonical path and its name.
    fn session_name(worktree: &str) -> Result<(PathBuf, String)> {
        let path = std::fs::canonicalize(worktree)
            .with_context(|| format!("finding the worktree {worktree}"))?;
        let name = format!(
            "{SESSION}{}",
            path.to_str()
                .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))?
        );
        Ok((path, name))
    }

    /// The place a session's record names, as its runs were given it.
    fn session_place(&self, name: &str, made: &Made) -> Result<Place> {
        let mut env = made.ports.clone();
        env.insert(
            "SLUICE_PLACE".to_string(),
            format!(
                "sluice-{}",
                &self.hash(&self.record(name).to_string_lossy())?[..10]
            ),
        );
        Ok(Place {
            name: name.into(),
            path: made
                .path
                .clone()
                .ok_or_else(|| anyhow!("place {name} names no worktree"))?,
            tmp: self.scratch(name, made),
            env,
        })
    }

    /// A preview as its record holds it: gone, while its command has not
    /// failed, as a candidate's place is ([`Places::folder_gone`]).
    fn preview_place(&self, name: &str, made: &Made) -> Result<PreviewPlace> {
        let path = made
            .path
            .clone()
            .unwrap_or_else(|| self.worktree_path(name));
        Ok(PreviewPlace {
            name: name.into(),
            branch: made.branch.clone().unwrap_or_default(),
            revision: made.revision.clone().unwrap_or_else(|| Rev::new("")),
            gone: made.failed.is_none() && self.folder_gone(&path)?,
            path,
            ports: made.ports.clone(),
            names: made.names.clone(),
            addresses: made.addresses.clone(),
            made: made.last_run.clone().unwrap_or_default(),
            run: made.run.clone().map(RunHandle),
            failed: made.failed.clone(),
            release_failed: made.release_failed.clone(),
        })
    }

    /// The place a preview's command runs in, as its record names it.
    fn preview_run_place(&self, name: &str, made: &Made) -> Result<Place> {
        let mut env = made.ports.clone();
        env.extend(made.names.clone());
        env.insert(
            "SLUICE_PLACE".to_string(),
            format!(
                "sluice-{}",
                &self.hash(&self.record(name).to_string_lossy())?[..10]
            ),
        );
        if let Some(b) = &made.branch {
            env.insert("SLUICE_BRANCH".into(), b.clone());
        }
        if let Some(r) = &made.revision {
            env.insert("SLUICE_REVISION".into(), r.to_string());
        }
        Ok(Place {
            name: name.into(),
            path: made
                .path
                .clone()
                .unwrap_or_else(|| self.worktree_path(name)),
            tmp: self.scratch(name, made),
            env,
        })
    }

    /// Every record in `.previews/`, by name.
    fn preview_records(&self) -> Result<Vec<(String, Made)>> {
        let dir = self.root().join(".previews");
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
        };
        let mut out = Vec::new();
        for e in entries {
            let path = e?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let made: Made = match std::fs::read(&path) {
                Ok(b) => serde_json::from_slice(&b)
                    .with_context(|| format!("reading {}", path.display()))?,
                // Released meanwhile.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if made.kind != Kind::Preview {
                continue;
            }
            let Some(rev) = &made.revision else { continue };
            let name = format!("{PREVIEW}{}", &rev.as_str()[..rev.as_str().len().min(12)]);
            out.push((name, made));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Previews-1 clause 99: a name for each of `ports` of a preview of
    /// `rev` on `branch`, none the same as another preview's.
    fn preview_names(
        &self,
        ports: &Ports,
        rev: &Rev,
        branch: &str,
    ) -> Result<BTreeMap<String, String>> {
        let taken: std::collections::BTreeSet<String> = self
            .preview_records()?
            .into_iter()
            .filter(|(_, m)| m.revision.as_ref() != Some(rev))
            .flat_map(|(_, m)| m.names.into_values())
            .collect();
        let repository = repository(&self.common);
        let make = |len: usize| -> Result<BTreeMap<String, String>> {
            let short = &rev.as_str()[..rev.as_str().len().min(len)];
            let mut names = BTreeMap::new();
            for p in &ports.names {
                let whole = format!("{p}\0{}\0{branch}\0{repository}", rev.as_str());
                let hash = &self.hash(&whole)?[..6];
                names.insert(name_env(p), label(p, short, branch, &repository, hash));
            }
            Ok(names)
        };
        let names = make(7)?;
        if names.values().any(|n| taken.contains(n)) {
            return make(12);
        }
        Ok(names)
    }

    /// Let go of what a place held beyond its checkout: the scratch
    /// directory its record `made` names, the ports it names and its
    /// record. With no record, no scratch is removed: one no record names
    /// is [`Checkout::remove_unnamed_scratch`]'s (123, rule core.30).
    fn let_go(&self, name: &str, made: Option<&Made>) -> Result<()> {
        if let Some(made) = made {
            let tmp = self.scratch(name, made);
            if tmp.exists() {
                std::fs::remove_dir_all(&tmp)?;
            }
        }
        if let Some(made) = made.filter(|m| !m.ports.is_empty()) {
            free_ports(&self.ports()?, &made.ports, &self.record(name))?;
        }
        // A session's record is removed under the lock `ran` writes it
        // under.
        let _lock = match crate::runs::is_session(name) {
            true => Some(self.lock_records()?),
            false => None,
        };
        match std::fs::remove_file(self.record(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// 123: keep on the place `name`'s record, while it is there, that its
    /// release failed with `e`: one more try, held once `stop` says so of
    /// its tries, and held still once held.
    fn keep_failure(&self, name: &str, e: &anyhow::Error, stop: impl Fn(u32) -> bool) {
        let kept = (|| -> Result<()> {
            let _lock = self.lock_records()?;
            let Some(mut made) = self.read_record(name)? else {
                return Ok(());
            };
            let before = made.release_failed.take();
            let tries = before.as_ref().map_or(0, |f| f.tries) + 1;
            made.release_failed = Some(ReleaseFailure {
                failed: format!("{e:#}"),
                tries,
                at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                stopped: before.is_some_and(|f| f.stopped) || stop(tries),
            });
            self.write_record(name, &made)
        })();
        if let Err(k) = kept {
            tracing::warn!(
                sluice.place = name,
                "keeping a failed release on its place failed: {k:#}"
            );
        }
    }

    /// Session-runs-1 clause 95: release the session's place `name`, held
    /// as its record `made` says: every run in it ended, the release
    /// command run once, as `policy` states it or, given none, as its last
    /// run carried it, then its scratch, ports and record let go. A release
    /// that fails keeps the place, held (123).
    fn release_held(
        &self,
        name: &str,
        made: &Made,
        policy: Option<&ReleasePolicy>,
    ) -> Result<Released> {
        if made.kind != Kind::Session {
            bail!("place {name} is a candidate's, not a session's");
        }
        let released = (|| {
            // 95: the policy's release command, built now, under the
            // executor its runs had; the last run's only when no policy is
            // given.
            let (with, released) = match policy {
                None => (ReleaseWith::LastRun, Released::LastRun),
                Some(p) => {
                    let place = self.session_place(name, made)?;
                    let request = self.build_release(&place, p, name)?;
                    (ReleaseWith::Fresh(request), Released::Policy)
                }
            };
            self.runs.remove_all_with(name, with)?;
            self.let_go(name, Some(made))?;
            Ok(released)
        })();
        // 123: held at once: no round releases a session's place (25).
        if let Err(e) = &released {
            self.keep_failure(name, e, |_| true);
        }
        released
    }

    /// Session-runs-1 clause 95, rule core.10, record 0120: release `name`,
    /// a session's place with no record, under `policy`, where its folder
    /// is a worktree of the repository: by the path a held place's release
    /// takes, with a record and a scratch of the release's own, the
    /// policy's branch on it and no ports, so the scratch is named while it
    /// exists (rule core.30) and the release is given the `SLUICE_PLACE` a
    /// run there is. Both are let go once the release ends, whether it
    /// passed or failed, so nothing is kept for the worktree and a release
    /// asked again runs again. A policy that names no release command, a
    /// folder that has gone and one that is no worktree of the repository,
    /// another repository's among them, have nothing to release.
    fn release_unheld(&self, name: &str, policy: &ReleasePolicy) -> Result<Released> {
        let Some(folder) = name.strip_prefix(SESSION) else {
            return Ok(Released::Nothing);
        };
        let Ok((path, _)) = Places::session_name(folder) else {
            return Ok(Released::Nothing);
        };
        if !self.listed(&path)? {
            return Ok(Released::Nothing);
        }
        let made = {
            let lock = self.lock_records()?;
            if let Some(made) = self.read_record(name)? {
                // Adopted meanwhile: held now, and released as held.
                drop(lock);
                return self.release_held(name, &made, Some(policy));
            }
            let made = Made {
                branch: Some(policy.branch.clone()),
                path: Some(path),
                scratch: Some(self.new_scratch(name)?),
                ..Made::new(Kind::Session, "session")
            };
            self.write_record(name, &made)?;
            made
        };
        let released = (|| {
            self.make_scratch(&self.scratch(name, &made))?;
            let place = self.session_place(name, &made)?;
            let Some(request) = self.build_release(&place, policy, name)? else {
                return Ok(Released::Nothing);
            };
            self.runs
                .remove_all_with(name, ReleaseWith::Fresh(Some(request)))?;
            Ok(Released::Policy)
        })();
        // Record 0120: nothing kept for it, passed or failed; a failed
        // release's run is no hold either.
        if released.is_err() {
            if let Err(e) = self.runs.discard_all(name) {
                tracing::warn!(
                    sluice.place = name,
                    "removing a failed release's run failed: {e:#}"
                );
            }
        }
        let freed = self.let_go(name, Some(&made));
        match (released, freed) {
            (Err(e), Err(f)) => {
                tracing::warn!(
                    sluice.place = name,
                    "letting go of a failed release's record failed: {f:#}"
                );
                Err(e)
            }
            (released, freed) => freed.and(released),
        }
    }

    /// 95: the release of `place`, the session's place `name`, as `policy`
    /// states it, built by the check seam's builder; `None` where the
    /// policy names no release command.
    fn build_release(
        &self,
        place: &Place,
        policy: &ReleasePolicy,
        name: &str,
    ) -> Result<Option<Request>> {
        let build = self
            .release
            .as_ref()
            .ok_or_else(|| anyhow!("no release command builder is bound"))?;
        build(
            place,
            &policy.tests,
            &policy.limits,
            &self.runs.release_dir(name),
        )
    }

    /// Records 0120 and 0115: whether git lists the folder `path` as one
    /// of the repository's worktrees, detached or not: its main worktree
    /// where it has one, or the folder an entry under `<common>/worktrees/`
    /// names. Read from the repository's files as `git worktree list` reads
    /// them, with no process started and no object read (129; rule
    /// core.31), each folder matched by what it is on disk ([`Folder`]),
    /// never by how its path is spelled. An entry whose `gitdir` file
    /// cannot be read, or whose folder cannot be, names nothing; worktrees
    /// that cannot be listed and a `path` that cannot be read are an error,
    /// never read as unlisted.
    fn listed(&self, path: &Path) -> Result<bool> {
        let want = Folder::of(path).with_context(|| format!("reading {}", path.display()))?;
        let repo = self.repo()?;
        let is = |dir: &Path| Folder::of(dir).is_ok_and(|f| f == want);
        if !repo.is_bare() && repo.workdir().is_some_and(is) {
            return Ok(true);
        }
        let linked = repo
            .worktrees()
            .map_err(|e| anyhow!("listing the worktrees: {e}"))?;
        Ok(linked.into_iter().any(|w| w.base().is_ok_and(|b| is(&b))))
    }

    /// 123, record 0115: whether the folder `path` of a place sluice made,
    /// a candidate's or a preview's, is gone: no directory, or one git no
    /// longer lists as a worktree, whatever is left in it.
    fn folder_gone(&self, path: &Path) -> Result<bool> {
        Ok(!path.is_dir() || !self.listed(path)?)
    }

    /// Hold ports in `dir` rather than the host's own.
    pub fn with_ports(mut self, dir: PathBuf) -> Self {
        self.ports = Some(dir);
        self
    }

    fn ports(&self) -> Result<PathBuf> {
        match &self.ports {
            Some(d) => Ok(d.clone()),
            None => ports_dir(),
        }
    }

    /// Keep the scratch of checkouts under `dir`, standing for the user's
    /// own directory, rather than under [`user_scratch`].
    pub fn with_scratch_root(mut self, dir: PathBuf) -> Self {
        self.scratch_root = Some(dir);
        self
    }

    /// The user's own directory, or the one standing for it.
    fn user_dir(&self) -> Result<PathBuf> {
        match &self.scratch_root {
            Some(d) => Ok(d.clone()),
            None => user_scratch(),
        }
    }

    /// The repository's directory under the user's.
    fn repository_dir(&self) -> Result<PathBuf> {
        let key = self.key.get_or_init(|| repository_key(&self.common));
        Ok(self.user_dir()?.join(key))
    }

    fn root(&self) -> PathBuf {
        self.common.join("sluice")
    }

    /// The repository's lock over its session places' records, beside
    /// them in its git directory, which every check that releases a place
    /// may write: held across keeping a result and across a release's
    /// removal.
    fn lock_records(&self) -> Result<File> {
        let root = self.root();
        std::fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        let lock = File::create(root.join(".places.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    fn record(&self, name: &str) -> PathBuf {
        self.root()
            .join(places_dir(name))
            .join(format!("{}.json", flat_name(name)))
    }

    /// A `git-worktree` place's checkout.
    fn worktree_path(&self, name: &str) -> PathBuf {
        self.root().join(places_dir(name)).join(dir_name(name))
    }

    /// 35, rule core.30: the scratch directory of the place `name`, as its
    /// record `made` names it; a record an older sluice wrote names none,
    /// and its scratch is where that sluice put it, the place's name as one
    /// directory name.
    fn scratch(&self, name: &str, made: &Made) -> PathBuf {
        made.scratch
            .clone()
            .unwrap_or_else(|| self.root().join(".tmp").join(dir_name(name)))
    }

    /// 35 as amended, rule core.30: where a place made now keeps its
    /// scratch, which its record names before the directory is made: the
    /// repository's directory under the user's, then [`scratch_name`].
    fn new_scratch(&self, name: &str) -> Result<PathBuf> {
        Ok(self.repository_dir()?.join(scratch_name(name)))
    }

    /// 35 as amended, rule core.30: make the scratch `path` its record
    /// names. Under the user's directory, that directory and each under it
    /// above the scratch, the repository's, are made the user's own first,
    /// or refused; a scratch elsewhere, under the repository's
    /// `sluice/.tmp/` for a place made before, is made as it was.
    fn make_scratch(&self, path: &Path) -> Result<()> {
        self.make_parents(path)?;
        std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))
    }

    /// The directories above the scratch `path`: under the user's
    /// directory, that directory and each under it, made the user's own,
    /// or refused; none for a scratch elsewhere.
    fn make_parents(&self, path: &Path) -> Result<()> {
        let user = self.user_dir()?;
        if let Some(parent) = path.parent().filter(|p| p.starts_with(&user)) {
            let mut dir = user.clone();
            make_own(&dir)?;
            for part in parent
                .strip_prefix(&user)
                .expect("under the user's directory")
            {
                dir.push(part);
                make_own(&dir)?;
            }
        }
        Ok(())
    }

    /// 35 as amended, rule core.30 (record 0063 as amended 2026-11-05):
    /// the former scratch of the session's place `name`, whose record
    /// `made` names one outside the user's directory or none, when none of
    /// its runs is going; none otherwise. A run going was given the former
    /// scratch, which its sandbox alone writes, and keeps it to its end.
    fn former_scratch(&self, name: &str, made: &Made) -> Result<Option<PathBuf>> {
        let tmp = self.scratch(name, made);
        if tmp.starts_with(self.user_dir()?) || self.runs.any_going(name)? {
            return Ok(None);
        }
        Ok(Some(tmp))
    }

    /// 35 as amended, rule core.30 (record 0063 as amended 2026-11-05):
    /// give a session's place the scratch `to` its record now names, in
    /// place of its former scratch `from`: `from` renamed to `to`, what
    /// its checks keep between runs going with it, or, where `from` is
    /// gone, `to` made. Where the system cannot rename it, from one file
    /// system to another or onto a directory that is not empty, `to` is
    /// made and `from` removed, since a check keeps nothing in its scratch
    /// it cannot make again; one that cannot be removed is warned of and
    /// left to [`Places::remove_unnamed`], since no record names it.
    fn move_scratch(&self, from: &Path, to: &Path) -> Result<()> {
        self.make_parents(to)?;
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(
                    "moving the scratch {} to {}: {e}; it is removed and {} made",
                    from.display(),
                    to.display(),
                    to.display()
                );
                if let Err(e) = std::fs::remove_dir_all(from) {
                    tracing::warn!("removing {}, which no checkout names: {e}", from.display());
                }
            }
        }
        std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))
    }

    /// 35 as amended, 123, rule core.30: every directory in the
    /// repository's two scratch roots as they are now, what
    /// [`Places::remove_unnamed`] may remove: the repository's directory
    /// under the user's, and the former `<common>/sluice/.tmp/`. The
    /// user's directory is read before anything under it is listed, and
    /// where it is not the user's own, closed to everyone else, it is
    /// warned of in the words its making refuses it in and nothing under
    /// it is listed. The user's directory itself is never listed, so no
    /// other repository's scratch is weighed.
    fn list_scratch(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let user = self.user_dir()?;
        match std::fs::symlink_metadata(&user) {
            Ok(meta) => match is_own(&user, &meta) {
                Ok(()) => list_dirs(&self.repository_dir()?, &mut out)?,
                Err(e) => tracing::warn!("{e:#}; nothing under it is removed"),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("reading {}", user.display())),
        }
        list_dirs(&self.root().join(".tmp"), &mut out)?;
        Ok(out)
    }

    /// What every record under `.places/` or `.previews/` names of the
    /// repository's scratch roots: its `scratch`, or, for a record naming none, its
    /// former path, from the place's name as `session_places`,
    /// `preview_records` and `made` read it back. A record that exists and
    /// cannot be read names its scratch still (35, rule core.30): it is
    /// warned of with the reader's words, and the place its file name
    /// reads back as keeps its scratch of either form, or, for a file name
    /// cut short, every directory is kept.
    fn named_scratch(&self) -> Result<Named> {
        let mut named = Named::default();
        for dir in [".places", ".previews"] {
            let dir = self.root().join(dir);
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
            };
            for e in entries {
                let path = e?.path();
                let Some(stem) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".json"))
                else {
                    continue;
                };
                let read = match std::fs::read(&path) {
                    // Released meanwhile.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    read => read
                        .map_err(anyhow::Error::from)
                        .and_then(|b| Ok(serde_json::from_slice::<Made>(&b)?)),
                };
                let made = match read {
                    Ok(made) => made,
                    Err(e) => {
                        tracing::warn!(
                            "reading {}, whose checkout's scratch is kept: {e:#}",
                            path.display()
                        );
                        match read_name(stem) {
                            Some(name) => named
                                .dirs
                                .extend([scratch_name(&name), dir_name(&name)].map(OsString::from)),
                            None => {
                                named.former = true;
                                named.short = true;
                            }
                        }
                        continue;
                    }
                };
                let name = match made.kind {
                    Kind::Session => made
                        .path
                        .as_ref()
                        .and_then(|p| p.to_str())
                        .map(|p| format!("{SESSION}{p}")),
                    Kind::Preview => made.revision.as_ref().map(|rev| {
                        format!("{PREVIEW}{}", &rev.as_str()[..rev.as_str().len().min(12)])
                    }),
                    Kind::Made => read_name(stem),
                };
                let scratch = match (&made.scratch, name) {
                    (Some(scratch), _) => scratch.clone(),
                    (None, Some(name)) => self.scratch(&name, &made),
                    (None, None) => {
                        named.former = true;
                        continue;
                    }
                };
                named
                    .dirs
                    .extend(scratch.file_name().map(|n| n.to_os_string()));
            }
        }
        Ok(named)
    }

    /// 123, rule core.30: remove each of `listed`, the scratch roots'
    /// directories as [`Places::list_scratch`] found them, that no record
    /// names now, and return those removed. Records are read after the
    /// listing, and a place's record names its scratch before the
    /// directory is made, so a listed directory's record, if it has one,
    /// is read, and a directory made since is in no listing: no lock is
    /// held against a place being made. While a record naming no scratch
    /// cannot be named, only directories of the short form are removed,
    /// and while one that cannot be read cannot be named, none. One that
    /// cannot be removed is warned of and the rest go on.
    fn remove_unnamed(&self, listed: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        let named = self.named_scratch()?;
        let mut removed = Vec::new();
        for dir in listed {
            let Some(own) = dir.file_name().map(|n| n.to_os_string()) else {
                continue;
            };
            if named.keeps(&own) {
                continue;
            }
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => removed.push(dir),
                // Removed meanwhile, by its place's release.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!("removing {}, which no checkout names: {e}", dir.display())
                }
            }
        }
        Ok(removed)
    }

    fn read_record(&self, name: &str) -> Result<Option<Made>> {
        match std::fs::read(self.record(name)) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_record(&self, name: &str, made: &Made) -> Result<()> {
        let path = self.record(name);
        std::fs::create_dir_all(path.parent().expect("a record has a directory"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(made)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// `git` in the common dir, with nothing of a caller's repository or
    /// hooks: a hook's `GIT_DIR` would point it elsewhere.
    fn git(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .arg("-C")
            .arg(&self.common)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .stdin(Stdio::null())
            .output()
            .context("running git")
    }

    fn make_worktree(&self, name: &str, rev: &Rev) -> Result<PathBuf> {
        let path = self.worktree_path(name);
        let out = self.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--detach",
            path.to_str()
                .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))?,
            rev.as_str(),
        ])?;
        if !out.status.success() {
            bail!(
                "git worktree add {}: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(path)
    }

    /// git's hash of `text`: names stay the same across sluice's builds.
    fn hash(&self, text: &str) -> Result<String> {
        use std::io::Write as _;
        let mut child = Command::new("git")
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("running git hash-object")?;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(text.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "git hash-object: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 35: the checkout cache is gone, and so are the caches an earlier
    /// sluice saved, under a dotted name no branch's records can take. A
    /// directory another process is removing at once, or one that cannot
    /// be removed, leaves the place to be made all the same.
    fn remove_saved_caches(&self) {
        let saved = self.root().join(".caches");
        if let Err(e) = std::fs::remove_dir_all(&saved) {
            if e.kind() != std::io::ErrorKind::NotFound && saved.exists() {
                tracing::warn!(
                    "removing the caches an earlier sluice saved, {}: {e}",
                    saved.display()
                );
            }
        }
    }

    /// 34, record 0115: remove the folder `path` of a place sluice made, a
    /// candidate's or a preview's, which is all it is ever given (rule
    /// core.20). A folder git lists goes by `git worktree remove --force`,
    /// whether or not it is still there. One git no longer lists, as git
    /// leaves a folder whose removal it stopped part way and then refuses,
    /// goes by sluice's own removal ([`remove_leftover`]); nothing where it
    /// is not there either.
    fn remove_worktree(&self, path: &Path) -> Result<()> {
        if !self.listed(path)? {
            return remove_leftover(path).with_context(|| {
                format!(
                    "removing what is left of {}, which git no longer lists as a worktree",
                    path.display()
                )
            });
        }
        let out = self.git(&[
            "worktree",
            "remove",
            "--force",
            path.to_str()
                .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))?,
        ])?;
        if !out.status.success() {
            bail!(
                "git worktree remove {}: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

/// A folder as it is on disk, whatever spells its path: the device and
/// inode of the folder, or, where it no longer exists, of its nearest
/// ancestor that does, with the rest of its path below that ancestor. So
/// `/var/…` and `/private/var/…`, or a path spelled through a link, are
/// one folder, and a folder deleted is still the one its entry names
/// (record 0115).
#[derive(Debug, PartialEq, Eq)]
struct Folder {
    dev: u64,
    ino: u64,
    rest: PathBuf,
}

impl Folder {
    fn of(path: &Path) -> std::io::Result<Folder> {
        use std::os::unix::fs::MetadataExt as _;
        let mut at = path;
        let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
        loop {
            // The folder itself is not followed, as git never follows it; an
            // ancestor is, since the rest of the path is reached through it.
            let meta = if rest.is_empty() {
                std::fs::symlink_metadata(at)
            } else {
                std::fs::metadata(at)
            };
            match meta {
                Ok(m) => {
                    return Ok(Folder {
                        dev: m.dev(),
                        ino: m.ino(),
                        rest: rest.iter().rev().collect(),
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
                        return Err(e);
                    };
                    rest.push(name);
                    at = if parent.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        parent
                    };
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Record 0115: remove `path`, what is left of a folder git no longer
/// lists, walked without following links: each directory its owner cannot
/// write, read or search is made so first, since a check can leave a tree
/// read-only (a copy out of the Nix store, a module cache) and git's
/// removal and `remove_dir_all` both fail on it; then each entry is
/// removed, and the folder. A failure names the entry and what refused it.
/// Nothing where `path`, or an entry of it, is not there.
fn remove_leftover(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let gone = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if gone(&e) => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if !meta.is_dir() {
        return match std::fs::remove_file(path) {
            Err(e) if !gone(&e) => Err(e).with_context(|| format!("removing {}", path.display())),
            _ => Ok(()),
        };
    }
    let mode = meta.permissions().mode();
    if mode & 0o700 != 0o700 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o700))
            .with_context(|| format!("making {} its owner's to write", path.display()))?;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) if gone(&e) => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("listing {}", path.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("listing {}", path.display()))?;
        remove_leftover(&entry.path())?;
    }
    match std::fs::remove_dir(path) {
        Err(e) if !gone(&e) => Err(e).with_context(|| format!("removing {}", path.display())),
        _ => Ok(()),
    }
}

/// The worktree at `worktree` whose HEAD is the ref `head`, with the
/// branch it names and that branch's revision, as `git worktree list`
/// reads them: `None` when HEAD is detached or names no branch, and the
/// null revision for a branch not yet made.
fn on_branch(repo: &gix::Repository, head: &str, worktree: PathBuf) -> Result<Option<CheckedOut>> {
    let found = repo
        .try_find_reference(head)
        .map_err(|e| anyhow!("reading {head}: {e}"))?;
    let Some(found) = found else {
        return Ok(None);
    };
    let gix::refs::TargetRef::Symbolic(name) = found.target() else {
        return Ok(None);
    };
    let full = name.as_bstr().to_string();
    let Some(branch) = full.strip_prefix("refs/heads/") else {
        return Ok(None);
    };
    let at = match repo
        .try_find_reference(name)
        .map_err(|e| anyhow!("reading {full}: {e}"))?
    {
        Some(mut r) => r
            .peel_to_id()
            .map_err(|e| anyhow!("peeling {full}: {e}"))?
            .to_string(),
        None => gix::ObjectId::null(repo.object_hash()).to_string(),
    };
    Ok(Some(CheckedOut {
        worktree,
        branch: branch.to_string(),
        head: Rev(at),
    }))
}

/// The worktrees `git worktree list --porcelain -z` names with a branch
/// checked out: a bare entry, a detached one and one whose directory is
/// gone (`prunable`) are left out. What [`Checkout::checked_out`] reads
/// with no process is this, which its tests compare it with.
pub fn parse_worktree_list(out: &[u8]) -> Vec<CheckedOut> {
    let mut found = Vec::new();
    let mut fields: Vec<&[u8]> = Vec::new();
    let mut flush = |fields: &mut Vec<&[u8]>| {
        let mut worktree = None;
        let mut head = None;
        let mut branch = None;
        let mut gone = false;
        for f in fields.drain(..) {
            let f = String::from_utf8_lossy(f);
            if let Some(w) = f.strip_prefix("worktree ") {
                worktree = Some(PathBuf::from(w));
            } else if let Some(h) = f.strip_prefix("HEAD ") {
                head = Some(Rev(h.to_string()));
            } else if let Some(b) = f.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            } else if f.starts_with("prunable") {
                gone = true;
            }
        }
        if let (Some(worktree), Some(head), Some(branch), false) = (worktree, head, branch, gone) {
            found.push(CheckedOut {
                worktree,
                branch,
                head,
            });
        }
    };
    for f in out.split(|b| *b == 0) {
        if f.is_empty() {
            flush(&mut fields);
        } else {
            fields.push(f);
        }
    }
    flush(&mut fields);
    found
}

/// The names of a preview begin so (previews-1 clause 98).
pub const PREVIEW: &str = "preview/";

/// Where a place's record and checkout are kept: a preview's apart, in
/// `.previews/`, which a sluice that predates previews never reads.
/// What a place's record says made it.
const METHOD: &str = "git-worktree";

/// `[checkout] method`, which a policy need not name, is the one there is
/// or is refused with the reason (ADR 0013).
fn one_method(checkout: &Settings) -> Result<()> {
    match checkout.get("method") {
        None => Ok(()),
        Some(Setting::Text(m)) if m == METHOD => Ok(()),
        Some(Setting::Text(m)) => bail!(
            "[checkout] method = {m:?}: a place is a git worktree, and [checkout] names no method (ADR 0013)"
        ),
        Some(_) => bail!("[checkout] method must be a string"),
    }
}

fn places_dir(name: &str) -> &'static str {
    if name.starts_with(PREVIEW) {
        ".previews"
    } else {
        ".places"
    }
}

impl Checkout for Places {
    fn materialize(&self, name: &str, rev: &Rev, checkout: &Settings) -> Result<Place> {
        one_method(checkout)?;
        let ports = Ports::read(checkout)?;
        self.remove_saved_caches();
        // A place of this name left by a step that stopped part way.
        if self.read_record(name)?.is_some() {
            self.release(name)?;
        }
        let record = self.record(name);
        let mut made = Made {
            scratch: Some(self.new_scratch(name)?),
            ..Made::new(Kind::Made, METHOD)
        };
        self.write_record(name, &made)?;
        let mut env = BTreeMap::from([(
            "SLUICE_PLACE".to_string(),
            format!("sluice-{}", &self.hash(&record.to_string_lossy())?[..10]),
        )]);
        if let Some(ports) = &ports {
            made.ports = hold_ports(&self.ports()?, ports, &record)?;
            self.write_record(name, &made)?;
            env.extend(made.ports.clone());
        }
        let tmp = self.scratch(name, &made);
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        self.make_scratch(&tmp)?;
        let path = self.make_worktree(name, rev)?;
        made.path = Some(path.clone());
        self.write_record(name, &made)?;
        Ok(Place {
            name: name.into(),
            path,
            tmp,
            env,
        })
    }

    /// Every candidate's place recorded under `.places/`, its name read
    /// back from its record's file name.
    fn made(&self) -> Result<Vec<String>> {
        let dir = self.root().join(".places");
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
        };
        let mut out = Vec::new();
        for e in entries {
            let path = e?.path();
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            let Some(name) = read_name(stem) else {
                continue;
            };
            let made: Made = match std::fs::read(&path) {
                Ok(b) => serde_json::from_slice(&b)
                    .with_context(|| format!("reading {}", path.display()))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if made.kind != Kind::Made {
                continue;
            }
            out.push(name);
        }
        out.sort();
        Ok(out)
    }

    fn release(&self, name: &str) -> Result<()> {
        if crate::runs::is_session(name) {
            self.release_session(name, None)?;
            return Ok(());
        }
        if name.starts_with(PREVIEW) {
            self.release_preview(name, None, 0)?;
            return Ok(());
        }
        // 34: everything started in it ends first, its release command
        // included (ADR 0016).
        self.runs.remove(name)?;
        let made = self.read_record(name)?;
        if let Some(made) = made.as_ref().filter(|m| m.kind == Kind::Made) {
            match made.method.as_str() {
                METHOD => self.remove_worktree(&self.worktree_path(name))?,
                other => {
                    bail!("place {name} was made by {other:?}, which this sluice cannot remove")
                }
            }
        }
        self.let_go(name, made.as_ref())
    }

    fn gone(&self, name: &str) -> Result<bool> {
        let Some(made) = self.read_record(name)? else {
            return Ok(false);
        };
        let Some(path) = made.path.as_deref() else {
            return Ok(false);
        };
        match made.kind {
            // Record 0115: a session's worktree is no place sluice made, and
            // is gone only once it is no directory (rule core.16).
            Kind::Session => Ok(!path.is_dir()),
            Kind::Made | Kind::Preview => self.folder_gone(path),
        }
    }

    fn free(&self, name: &str) -> Result<()> {
        let made = self.read_record(name)?;
        // 123: everything started in it ends, and nothing runs in its
        // name: no release command. Record 0121: a preview's containers
        // end once its run has, while it holds its ports; one left keeps
        // the preview whole, its run's directory with it.
        if let Some(made) = made.as_ref().filter(|m| m.kind == Kind::Preview) {
            self.runs.end_keeping(name)?;
            self.end_containers(name, made)?;
        }
        self.runs.discard(name)?;
        if let Some(made) = made.as_ref().filter(|m| m.kind != Kind::Session) {
            match made.method.as_str() {
                METHOD => self.remove_worktree(&self.worktree_path(name))?,
                other => {
                    bail!("place {name} was made by {other:?}, which this sluice cannot remove")
                }
            }
        }
        // A session's worktree stays (25, rule core.16).
        self.let_go(name, made.as_ref())
    }

    fn release_gone(&self, name: &str) -> Result<GoneRelease> {
        // 123: noted before it starts, so it never starts twice; it runs in
        // the scratch the record names (rule core.30).
        let start = {
            let _lock = self.lock_records()?;
            match self.read_record(name)? {
                Some(mut made) if made.gone_release.is_none() => {
                    made.gone_release =
                        Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
                    self.write_record(name, &made)?;
                    Some(self.scratch(name, &made))
                }
                _ => None,
            }
        };
        let mut gone = GoneRelease::default();
        if let Some(tmp) = start {
            self.make_scratch(&tmp)?;
            match self.runs.release_gone(name, &tmp) {
                Ok(ran) => gone.ran = ran,
                Err(e) => {
                    gone.ran = true;
                    gone.failed = Some(format!("{e:#}"));
                }
            }
        }
        self.free(name)?;
        Ok(gone)
    }

    fn remove_unnamed_scratch(&self) -> Result<Vec<PathBuf>> {
        let listed = self.list_scratch()?;
        // 129: nothing in either scratch root to name, and no record read.
        if listed.is_empty() {
            return Ok(Vec::new());
        }
        self.remove_unnamed(listed)
    }

    fn adopted(&self, worktree: &str) -> Result<Option<Adopted>> {
        let Ok((_, name)) = Places::session_name(worktree) else {
            return Ok(None);
        };
        Ok(match self.read_record(&name)? {
            Some(made) if made.kind == Kind::Session => Some(Adopted {
                branch: made.branch.unwrap_or_default(),
                name,
            }),
            _ => None,
        })
    }

    fn release_session(&self, name: &str, policy: Option<&ReleasePolicy>) -> Result<Released> {
        let Some(made) = self.read_record(name)? else {
            // A stop part way may have left its runs: end them all the same.
            if crate::runs::is_session(name) {
                self.runs.remove_all_with(name, ReleaseWith::Fresh(None))?;
            }
            // Record 0120: a worktree sluice holds nothing for is released
            // by the policy's release command all the same.
            return match policy {
                Some(p) => self.release_unheld(name, p),
                None => Ok(Released::Nothing),
            };
        };
        self.release_held(name, &made, policy)
    }

    fn session_places(&self) -> Result<Vec<SessionPlace>> {
        let dir = self.root().join(".places");
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
        };
        let mut out = Vec::new();
        for e in entries {
            let path = e?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let made: Made = match std::fs::read(&path) {
                Ok(b) => serde_json::from_slice(&b)
                    .with_context(|| format!("reading {}", path.display()))?,
                // Released meanwhile.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if made.kind != Kind::Session {
                continue;
            }
            let Some(worktree) = made.path.clone() else {
                continue;
            };
            let name = format!(
                "{SESSION}{}",
                worktree
                    .to_str()
                    .ok_or_else(|| anyhow!("{} is not UTF-8", worktree.display()))?
            );
            // 97: a worktree that no longer exists is no session's.
            let gone = !worktree.is_dir();
            out.push(SessionPlace {
                name,
                gone,
                worktree,
                branch: made.branch.unwrap_or_default(),
                ports: made.ports,
                last_run: made.last_run,
                last_result: made.last_result,
                release_failed: made.release_failed,
            });
        }
        out.sort_by(|a, b| a.worktree.cmp(&b.worktree));
        Ok(out)
    }

    fn ran(&self, name: &str, last: &LastResult) -> Result<()> {
        // Under the lock `let_go` removes a session's record under, so a
        // release between the read and the write never writes a released
        // place's record again, freed ports and all.
        let _lock = self.lock_records()?;
        let mut made = match self.read_record(name)? {
            Some(m) if m.kind == Kind::Session => m,
            // Released meanwhile: there is no place to keep it on (95).
            None => return Ok(()),
            Some(_) => bail!("place {name} is no session's"),
        };
        made.last_result = Some(last.clone());
        self.write_record(name, &made)
    }

    /// Every worktree with a branch checked out, read as `git worktree
    /// list` lists them with no process started (129): the main worktree
    /// where the repository is not bare, then each linked one whose
    /// directory is there or which is locked; one whose HEAD names no
    /// branch is left out, as [`parse_worktree_list`] leaves it.
    fn checked_out(&self) -> Result<Vec<CheckedOut>> {
        let repo = self.repo()?;
        let mut found = Vec::new();
        if !repo.is_bare() {
            if let Some(dir) = repo.workdir() {
                found.extend(on_branch(repo, "main-worktree/HEAD", dir.to_path_buf())?);
            }
        }
        let linked = repo
            .worktrees()
            .map_err(|e| anyhow!("listing the worktrees: {e}"))?;
        for w in linked {
            // Its directory gone, as `git worktree prune` would remove it.
            if w.is_prunable() {
                continue;
            }
            let (Ok(base), Ok(id)) = (w.base(), w.id()) else {
                continue;
            };
            found.extend(on_branch(repo, &format!("worktrees/{id}/HEAD"), base)?);
        }
        Ok(found
            .into_iter()
            .map(|mut c| {
                // As a session's place names its worktree (`session_name`).
                if let Ok(p) = std::fs::canonicalize(&c.worktree) {
                    c.worktree = p;
                }
                c
            })
            .collect())
    }

    fn adopt(&self, worktree: &str, branch: &str, checkout: &Settings, at: &str) -> Result<Place> {
        let (path, name) = Places::session_name(worktree)?;
        let record = self.record(&name);
        let (mut made, former) = match self.read_record(&name)? {
            Some(made) if made.kind == Kind::Session => {
                let former = self.former_scratch(&name, &made)?;
                (made, former)
            }
            Some(_) => bail!("place {name} is a candidate's, not a session's"),
            None => {
                let mut made = Made {
                    branch: Some(branch.into()),
                    path: Some(path.clone()),
                    scratch: Some(self.new_scratch(&name)?),
                    ..Made::new(Kind::Session, "session")
                };
                // Written before its ports are held, so a hold names a
                // record that exists, and a stop between leaves a record
                // release reads.
                self.write_record(&name, &made)?;
                if let Some(ports) = Ports::read(checkout)? {
                    made.ports = hold_ports(&self.ports()?, &ports, &record)?;
                    self.write_record(&name, &made)?;
                }
                (made, None)
            }
        };
        // 35 as amended, rule core.30: a place made before takes the short
        // path, its record naming it before its former scratch is moved.
        if former.is_some() {
            made.scratch = Some(self.new_scratch(&name)?);
        }
        made.last_run = Some(at.into());
        // 97: a run under way, or one whose starter was killed, has no
        // result; the one before is not this run's.
        made.last_result = None;
        self.write_record(&name, &made)?;
        let tmp = self.scratch(&name, &made);
        match &former {
            Some(from) => self.move_scratch(from, &tmp)?,
            None => self.make_scratch(&tmp)?,
        }
        let mut env = made.ports.clone();
        env.insert(
            "SLUICE_PLACE".to_string(),
            format!("sluice-{}", &self.hash(&record.to_string_lossy())?[..10]),
        );
        Ok(Place {
            name,
            path,
            tmp,
            env,
        })
    }

    fn preview(
        &self,
        name: &str,
        rev: &Rev,
        branch: &str,
        checkout: &Settings,
        addresses: &BTreeMap<String, String>,
        at: &str,
    ) -> Result<(Place, PreviewPlace)> {
        assert!(name.starts_with(PREVIEW), "{name} is no preview's name");
        // 100: one preview of a revision, given back as it is.
        if let Some(made) = self.read_record(name)? {
            if made.kind != Kind::Preview {
                bail!("place {name} is not a preview");
            }
            return Ok((
                self.preview_run_place(name, &made)?,
                self.preview_place(name, &made)?,
            ));
        }
        one_method(checkout)?;
        let ports = Ports::read(checkout)?;
        // 99: each address checked against the ports before anything is
        // made, with stand-in names and ports.
        let (mut names, mut env_ports) = (BTreeMap::new(), BTreeMap::new());
        for p in ports.iter().flat_map(|p| &p.names) {
            names.insert(name_env(p), String::new());
            env_ports.insert(env_name(p), String::new());
        }
        for (label, template) in addresses {
            fill(label, template, &names, &env_ports)?;
        }
        let record = self.record(name);
        let mut made = Made {
            branch: Some(branch.into()),
            last_run: Some(at.into()),
            revision: Some(rev.clone()),
            scratch: Some(self.new_scratch(name)?),
            ..Made::new(Kind::Preview, METHOD)
        };
        // Written before its ports are held, so a hold names a record that
        // exists, and a stop part way leaves a record release reads.
        self.write_record(name, &made)?;
        if let Some(ports) = &ports {
            made.names = self.preview_names(ports, rev, branch)?;
            made.ports = hold_ports(&self.ports()?, ports, &record)?;
            self.write_record(name, &made)?;
        }
        for (label, template) in addresses {
            made.addresses.insert(
                label.clone(),
                fill(label, template, &made.names, &made.ports)?,
            );
        }
        self.write_record(name, &made)?;
        let tmp = self.scratch(name, &made);
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        self.make_scratch(&tmp)?;
        let path = self.make_worktree(name, rev)?;
        made.path = Some(path);
        self.write_record(name, &made)?;
        Ok((
            self.preview_run_place(name, &made)?,
            self.preview_place(name, &made)?,
        ))
    }

    fn preview_run(&self, name: &str, run: &RunHandle) -> Result<()> {
        let mut made = match self.read_record(name)? {
            Some(m) if m.kind == Kind::Preview => m,
            _ => bail!("no preview {name}"),
        };
        made.run = Some(run.0.clone());
        self.write_record(name, &made)
    }

    fn previews(&self) -> Result<Vec<PreviewPlace>> {
        self.preview_records()?
            .iter()
            .map(|(name, made)| self.preview_place(name, made))
            .collect()
    }

    fn release_preview(&self, name: &str, failed: Option<&str>, retries: u32) -> Result<bool> {
        let Some(made) = self.read_record(name)? else {
            // A stop part way may have left its run: end it all the same.
            self.runs.remove(name)?;
            return Ok(false);
        };
        if made.kind != Kind::Preview {
            bail!("place {name} is not a preview");
        }
        // 34: everything started in it ends, its release command included
        // (ADR 0016), then the containers it left (record 0121), before its
        // run's directory, its checkout and its ports go; 123: a release
        // command that fails, or a container left, keeps the preview, held
        // once its tries are spent, and a later try runs no release command
        // that finished.
        if let Err(e) = self
            .runs
            .release_keeping(name)
            .and_then(|()| self.end_containers(name, &made))
            .and_then(|()| self.runs.remove(name))
        {
            self.keep_failure(name, &e, |tries| tries > retries);
            return Err(e);
        }
        self.remove_worktree(&self.worktree_path(name))?;
        let tmp = self.scratch(name, &made);
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        free_ports(&self.ports()?, &made.ports, &self.record(name))?;
        match failed {
            // 101: kept, holding nothing, with what its command printed.
            Some(output) if made.failed.is_none() => self.write_record(
                name,
                &Made {
                    branch: made.branch.clone(),
                    last_run: made.last_run.clone(),
                    revision: made.revision.clone(),
                    names: made.names.clone(),
                    addresses: made.addresses.clone(),
                    failed: Some(output.into()),
                    ..Made::new(Kind::Preview, &made.method)
                },
            )?,
            _ => match std::fs::remove_file(self.record(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            },
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fill, free_ports, hold_ports, label, own_dir, port_free, repository, repository_key,
        scratch_name, slug, user_scratch, Kind, Made, Places, Ports,
    };
    use crate::runs::{dir_name, Runs};
    use sluice_seams::{Checkout, LastResult, Rev, RunKind, RunOutcome};
    use std::collections::BTreeMap;
    use std::path::Path;

    fn last() -> LastResult {
        LastResult {
            at: "2026-10-16T09:00:00Z".into(),
            head: Rev("a".into()),
            kind: RunKind::Checks,
            outcome: RunOutcome::Pass,
            recorded: true,
            tasks: Vec::new(),
            reused: None,
        }
    }

    /// A directory of the test's own for held ports and the records that
    /// hold them, and the first of `n` free ports from `from`, so tests
    /// beside each other and the host's own listeners keep apart.
    fn ports_scratch(test: &str, from: u32, n: u32) -> (std::path::PathBuf, u32) {
        let root = std::env::temp_dir().join(format!("sluice-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let from = from + std::process::id() % 500;
        let base = (from..60000)
            .find(|b| (*b..*b + n).all(|p| port_free(p as u16)))
            .expect("a free run of ports");
        (root, base)
    }

    fn ports(n: usize, from: u32) -> Ports {
        Ports {
            names: (0..n).map(|i| format!("p{i}")).collect(),
            from,
        }
    }

    fn record(root: &Path, name: &str) -> std::path::PathBuf {
        let r = root.join(format!("{name}.json"));
        std::fs::write(&r, "{}").unwrap();
        r
    }

    fn held(ports: &BTreeMap<String, String>) -> std::collections::BTreeSet<u32> {
        ports.values().map(|p| p.parse().unwrap()).collect()
    }

    /// 31, 32, record 0016 as amended 2026-11-13: a place of a policy that
    /// names 19 ports, made beside one of a policy that names 21, holds no
    /// port the first holds.
    #[test]
    fn policies_of_different_lengths_hold_no_port_twice() {
        let (root, base) = ports_scratch("ports-lengths", 30000, 60);
        let dir = root.join("ports");
        let a = hold_ports(&dir, &ports(21, base), &record(&root, "a")).unwrap();
        assert_eq!(held(&a), (base..base + 21).collect());
        let b = hold_ports(&dir, &ports(19, base), &record(&root, "b")).unwrap();
        assert_eq!(b.len(), 19);
        assert!(held(&a).is_disjoint(&held(&b)), "{a:?} {b:?}");
        assert_eq!(held(&b), (base + 21..base + 40).collect());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 31, record 0016 as amended 2026-11-13: the ports of a place whose
    /// record is gone are free again, and freed ports too.
    #[test]
    fn a_gone_places_ports_are_free_again() {
        let (root, base) = ports_scratch("ports-gone", 33000, 10);
        let dir = root.join("ports");
        let first = record(&root, "first");
        let a = hold_ports(&dir, &ports(3, base), &first).unwrap();
        assert_eq!(held(&a), (base..base + 3).collect());
        std::fs::remove_file(&first).unwrap();
        let second = record(&root, "second");
        let b = hold_ports(&dir, &ports(3, base), &second).unwrap();
        assert_eq!(held(&b), (base..base + 3).collect());
        // Freed by its release, they go to the next place.
        free_ports(&dir, &b, &second).unwrap();
        let c = hold_ports(&dir, &ports(3, base), &record(&root, "third")).unwrap();
        assert_eq!(held(&c), (base..base + 3).collect());
        // A release of a place whose ports went to another frees none of
        // them.
        free_ports(&dir, &a, &first).unwrap();
        let d = hold_ports(&dir, &ports(3, base), &record(&root, "fourth")).unwrap();
        assert!(held(&c).is_disjoint(&held(&d)), "{c:?} {d:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 31, record 0016 as amended 2026-11-13: a port no place holds that a
    /// process listens on is passed by, with the run it would begin.
    #[test]
    fn a_port_in_use_is_passed_by() {
        let (root, base) = ports_scratch("ports-in-use", 36000, 10);
        let dir = root.join("ports");
        let _listening =
            std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, (base + 1) as u16))
                .unwrap();
        let a = hold_ports(&dir, &ports(2, base), &record(&root, "a")).unwrap();
        assert_eq!(held(&a), [base + 2, base + 3].into());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A place with no ports is released without touching the host's
    /// held ports, which a sandboxed check may not reach: a
    /// candidate's, and a session's.
    #[test]
    fn a_release_without_ports_never_takes_the_hosts_lock() {
        let root = std::env::temp_dir().join(format!("sluice-let-go-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("not-a-dir"), "").unwrap();
        let places = Places::new(&root, Runs::new(&root, "sluice".into()))
            .with_ports(root.join("not-a-dir/blocks"));
        let name = "try/main/90";
        let made = Made::new(Kind::Made, "worktree");
        places.write_record(name, &made).unwrap();
        places.let_go(name, Some(&made)).unwrap();
        assert!(!places.record(name).exists());
        let session = format!("session/{}", root.join("wt").display());
        let made = Made::new(Kind::Session, "session");
        places.write_record(&session, &made).unwrap();
        places.let_go(&session, Some(&made)).unwrap();
        assert!(!places.record(&session).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 95, 97: a result kept as a release removes the place's record waits
    /// for the host's lock, and writes nothing once the record is gone, so a
    /// released place is never written again.
    #[test]
    fn a_result_is_never_kept_on_a_released_place() {
        let root = std::env::temp_dir().join(format!("sluice-ran-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let open = || {
            Places::new(&root, Runs::new(&root, "sluice".into())).with_ports(root.join("blocks"))
        };
        let places = open();
        let name = format!("session/{}", root.join("wt").display());
        let record = places.record(&name);

        // No record: nothing to keep it on, and nothing made.
        places.ran(&name, &last()).unwrap();
        assert!(!record.exists());

        let made = Made {
            path: Some(root.join("wt")),
            ..Made::new(Kind::Session, "session")
        };
        places.write_record(&name, &made).unwrap();
        let held = places.lock_records().unwrap();
        std::thread::scope(|s| {
            let keeping = s.spawn(|| open().ran(&name, &last()));
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(
                places
                    .read_record(&name)
                    .unwrap()
                    .unwrap()
                    .last_result
                    .is_none(),
                "kept without the lock"
            );
            // A release removes the record under the lock.
            std::fs::remove_file(&record).unwrap();
            drop(held);
            keeping.join().unwrap().unwrap();
        });
        assert!(!record.exists(), "a released place's record written again");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 35: a record an older sluice wrote with the caches it kept reads,
    /// and is written back without them.
    #[test]
    fn an_older_records_caches_are_read_and_dropped() {
        let made: Made = serde_json::from_str(
            r#"{"kind":"made","method":"git-worktree","path":"/p",
                "caches":[{"paths":["target"],"bucket":"b","key":"k"}]}"#,
        )
        .unwrap();
        assert_eq!(made.path.as_deref(), Some(Path::new("/p")));
        let text = serde_json::to_string(&made).unwrap();
        assert!(!text.contains("caches"), "{text}");
    }

    /// 35, rule core.30: a scratch's name is 10 lower-case hex digits, one
    /// name's always the same and two names' different, and never a
    /// former scratch's name, which holds `+2F` or `+~`.
    #[test]
    fn a_scratch_name_is_ten_hex_digits_of_the_name() {
        let names = [
            "sluice/try/main/39".to_string(),
            "session//Users/me/wt/a".to_string(),
            "preview/0123456789ab".to_string(),
            format!("session//{}", "w".repeat(300)),
        ];
        for name in &names {
            let short = scratch_name(name);
            assert_eq!(short.len(), 10, "{short}");
            assert!(
                short.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "{short}"
            );
            assert_eq!(short, scratch_name(name), "one name, two scratch names");
            let former = dir_name(name);
            assert!(former.contains("+2F") || former.contains("+~"), "{former}");
            assert_ne!(short, former);
        }
        assert_ne!(scratch_name(&names[0]), scratch_name(&names[1]));
    }

    /// 35 as amended, rule core.30: a repository's key is 10 lower-case
    /// hex digits of where the repository is, one path's always the same
    /// and two paths' different; a place's scratch is the user's
    /// directory, the key and the place's own 10, 22 bytes past the
    /// directory whatever the place is called.
    #[test]
    fn a_scratch_is_the_users_directory_the_repositorys_key_and_its_name() {
        let common = Path::new("/Users/me/code/sluice/.git");
        let key = repository_key(common);
        assert_eq!(key.len(), 10, "{key}");
        assert!(
            key.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{key}"
        );
        assert_eq!(key, repository_key(common), "one path, two keys");
        assert_ne!(key, repository_key(Path::new("/Users/me/code/other/.git")));

        let root = Path::new("/s");
        let places =
            Places::new(common, Runs::new(common, "sluice".into())).with_scratch_root(root.into());
        for name in [
            "sluice/try/main/39",
            &format!("session//{}", "w".repeat(300)),
        ] {
            let tmp = places.new_scratch(name).unwrap();
            assert_eq!(tmp, root.join(&key).join(scratch_name(name)));
            assert_eq!(
                tmp.as_os_str().len(),
                root.as_os_str().len() + 22,
                "{}",
                tmp.display()
            );
        }
    }

    /// 35 as amended, rule core.30: the user's directory is `/tmp`
    /// resolved and `sluice-<uid>`, as the sandbox matches paths.
    #[test]
    fn the_users_scratch_directory_is_resolved() {
        let dir = user_scratch().unwrap();
        assert_eq!(
            dir.parent(),
            Some(std::fs::canonicalize("/tmp").unwrap().as_path())
        );
        assert_eq!(
            dir.file_name().unwrap().to_string_lossy(),
            format!("sluice-{}", nix::unistd::geteuid())
        );
        #[cfg(target_os = "macos")]
        assert!(dir.starts_with("/private/tmp"), "{}", dir.display());
    }

    /// 35 as amended, rule core.30: a scratch is kept only under a
    /// directory, not a link, of the user's own and closed to everyone
    /// else; a link, a file, another user's directory and one with any
    /// permission for others are each refused with what is wrong.
    #[test]
    fn only_the_users_own_closed_directory_holds_a_scratch() {
        let root = std::env::temp_dir().join(format!("sluice-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("file"), "").unwrap();
        std::os::unix::fs::symlink(root.join("dir"), root.join("link")).unwrap();
        let kind = |p: &str| std::fs::symlink_metadata(root.join(p)).unwrap().file_type();
        let uid = 501;
        assert_eq!(own_dir(kind("dir"), uid, 0o40700, uid), Ok(()));
        assert_eq!(own_dir(kind("link"), uid, 0o700, uid), Err("is a link"));
        assert_eq!(
            own_dir(kind("file"), uid, 0o600, uid),
            Err("is not a directory")
        );
        assert_eq!(
            own_dir(kind("dir"), uid + 1, 0o700, uid),
            Err("belongs to another user")
        );
        for bit in [0o001, 0o002, 0o004, 0o010, 0o020, 0o040] {
            assert_eq!(
                own_dir(kind("dir"), uid, 0o700 | bit, uid),
                Err("may be entered by others"),
                "{bit:o}"
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 35, rule core.30: a record an older sluice wrote names no scratch.
    #[test]
    fn an_older_record_names_no_scratch() {
        let made: Made =
            serde_json::from_str(r#"{"kind":"session","method":"session","path":"/w"}"#).unwrap();
        assert_eq!(made.scratch, None);
        let text = serde_json::to_string(&made).unwrap();
        assert!(!text.contains("scratch"), "{text}");
    }

    /// 35 as amended, 123, rule core.30: a place made after the scratch
    /// roots were listed keeps its scratch, its record written first; a
    /// directory either listing held that no record names is removed.
    #[test]
    fn a_scratch_made_after_the_listing_is_kept() {
        let root = std::env::temp_dir().join(format!("sluice-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let places = Places::new(&root, Runs::new(&root, "sluice".into()))
            .with_scratch_root(root.join("user"));
        let users = places.repository_dir().unwrap().join("abcdef0123");
        places.make_scratch(&users).unwrap();
        let former = root.join("sluice/.tmp/0123456789");
        std::fs::create_dir_all(&former).unwrap();
        let listed = places.list_scratch().unwrap();
        assert_eq!(listed, vec![users.clone(), former.clone()]);

        let name = "sluice/try/main/7";
        let made = Made {
            scratch: Some(places.new_scratch(name).unwrap()),
            ..Made::new(Kind::Made, "git-worktree")
        };
        places.write_record(name, &made).unwrap();
        let tmp = places.scratch(name, &made);
        places.make_scratch(&tmp).unwrap();

        assert_eq!(
            places.remove_unnamed(listed).unwrap(),
            vec![users.clone(), former.clone()]
        );
        assert!(tmp.is_dir(), "a scratch made after the listing was removed");
        assert!(!users.exists() && !former.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 99: a name is one label of lower-case letters, digits and hyphens.
    #[test]
    fn a_slug_is_part_of_a_label() {
        assert_eq!(slug("Bolt/The_Edge--signs"), "bolt-the-edge-signs");
        assert_eq!(slug("/x/"), "x");
        assert_eq!(slug("API_Server"), "api-server");
    }

    /// 99: a short name is whole; a long one keeps the port and the
    /// revision and is cut to 63 with a hash; a slash and capitals fold.
    #[test]
    fn a_name_is_one_label_of_at_most_63() {
        assert_eq!(
            label("web", "abc1234", "bolt/x", "sluice", "3f9a1c"),
            "web-abc1234--bolt-x--sluice"
        );
        let long = label(
            "portal",
            "abc1234",
            "bolt/the-local-world-finishes-the-real-deploy",
            "switchboard",
            "3f9a1c",
        );
        assert_eq!(long.len(), 63, "{long}");
        assert!(
            long.starts_with("portal-abc1234--bolt-the-local-world"),
            "{long}"
        );
        assert!(long.ends_with("-3f9a1c"), "{long}");
        assert!(!long.contains("---"), "{long}");
        let other = label(
            "portal",
            "abc1234",
            "bolt/the-local-world-finishes-the-real-deploy-again",
            "switchboard",
            "77e0d2",
        );
        assert_ne!(long, other, "two long names that share a beginning");
        let caps = label("Web_Server", "ABC1234", "Unit/X", "Repo.Name", "000000");
        assert_eq!(caps, "web-server-ABC1234--unit-x--repo-name");
    }

    /// 99: the repository's name, from its common directory.
    #[test]
    fn the_repository_is_named_from_its_common_directory() {
        assert_eq!(repository(Path::new("/c/sluice/sluice.git")), "sluice");
        assert_eq!(repository(Path::new("/c/switchboard/.git")), "switchboard");
        assert_eq!(repository(Path::new("/c/breadboard/.bare")), "breadboard");
        assert_eq!(repository(Path::new("/c/shop/git")), "shop");
    }

    /// 99: an address is filled in with the names and ports; a port the
    /// policy does not list is refused, naming the address.
    #[test]
    fn an_address_is_filled_in() {
        let names = BTreeMap::from([("SLUICE_NAME_WEB".to_string(), "web-a--b--c".to_string())]);
        let ports = BTreeMap::from([("SLUICE_PORT_WEB".to_string(), "20000".to_string())]);
        assert_eq!(
            fill(
                "web",
                "https://{name.web}.localhost:{port.web}/x",
                &names,
                &ports
            )
            .unwrap(),
            "https://web-a--b--c.localhost:20000/x"
        );
        let e = fill("api", "https://{name.api}.localhost", &names, &ports).unwrap_err();
        assert!(
            format!("{e}").contains("[preview] addresses api names {name.api}"),
            "{e}"
        );
    }
}
