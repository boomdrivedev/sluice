//! The branch seam's `pushed refs` profile (C.4, C.7 `remote`): a branch
//! whose shared copy lives on a git remote. Every read fetches it; a
//! landing is a push leased on the revision the combination was formed on
//! (24, 45, 46). Forming and committing stay local refs: every object is
//! on this host.
//!
//! Reading writes only the remote-tracking ref, never the local branch,
//! so the submit operation, which reads the branch, moves no branch
//! (rule core.4). The local branch is sluice's copy of the remote's: a
//! landing's push leaves it, and only a round levels it (124). The git
//! command fetches and pushes, carrying the
//! host's credential helpers and SSH setup, or, where sluice has an
//! [`Identity`] on the remote's forge, that identity's credentials alone.
//! Every call it makes to the remote runs under the retry policy, which
//! tries again what the remote never judged, a refused credential bought
//! afresh, and never a push the remote answered (134; rule core.29).
//! One fetch at a time holds the repository's fetch lock, and a process
//! that fetch's git runs, the `reference-transaction` hook above all,
//! fetches nothing and waits on no lock of its ancestor's (record 0110).
//! No try outlasts the policy's limit for one: git is ended with
//! everything it started, and a fetch's lock goes with it (record 0109).
//!
//! The remote's address is read as the host's configuration holds it when
//! each call begins, never kept from the profile's opening: a driver that
//! runs for days reaches a remote pointed elsewhere since, as at a
//! repository renamed on its forge, at its next call.
//!
//! A landing commit names whoever pushes it, read as the commit is
//! written: the author the identity names through its own interface, where
//! sluice reaches the remote as one, and `sluice <sluice@localhost>` where
//! it reaches it with the host's own credentials. An author the identity
//! cannot give is the commit's failure, and nothing is written. A
//! combination is written as sluice on every host, so one combination
//! forms one commit wherever it is formed (record 0019 as amended
//! 2026-11-18; rules core.1, core.2, core.6, core.43).

use crate::workers::Run as _;
use std::fs::TryLockError;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sluice_seams::{Branch, Formed, HostCopy, LandingCommit, Moved, ReadRefused, Rev};
use weir_bounded::span;

use super::local_refs::LocalRefs;
use crate::identity::{
    withheld, Author, Credentials, Identity, ASKED_VAR, PASSWORD_VAR, USERNAME_VAR,
};
use crate::retry::{Failed, Judged, Kind, Policy};

pub struct PushedRefs {
    local: LocalRefs,
    remote: String,
    /// sluice's identity, where it has one, and the helper command that
    /// gives git its credentials.
    identity: Option<(Rc<dyn Identity>, String)>,
    policy: Rc<Policy>,
}

/// sluice's identity on the remote as one call reaches it: the URL its
/// git reaches the remote by, read from the remote's address when the call
/// began, and the helper command that gives git its credentials.
struct Speaker<'a> {
    identity: &'a dyn Identity,
    url: String,
    helper: &'a str,
}

impl PushedRefs {
    /// The repository holding `path`, whose branches live on `remote`,
    /// reached with the host's own credentials under a command's retry
    /// policy. Refused when the repository has no remote of that name.
    pub fn open(path: &Path, remote: &str) -> Result<Self> {
        PushedRefs::open_as(path, remote, None, Rc::new(Policy::command()))
    }

    /// As [`PushedRefs::open`], reaching the remote as `identity` where it
    /// has one on the remote's forge, each call under `policy`; `program`
    /// is the `sluice` executable git runs as its credential helper.
    pub fn open_as(
        path: &Path,
        remote: &str,
        identity: Option<(Rc<dyn Identity>, PathBuf)>,
        policy: Rc<Policy>,
    ) -> Result<Self> {
        let local = LocalRefs::open(path)?;
        if !local.remotes()?.iter().any(|n| n == remote) {
            bail!("the policy names the remote {remote:?}, which this repository does not have");
        }
        Ok(PushedRefs {
            local,
            remote: remote.into(),
            identity: identity
                .map(|(identity, program)| (identity, format!("!{} credential", quoted(&program)))),
            policy,
        })
    }

    /// The repository's git directory.
    pub fn git_dir(&self) -> &Path {
        self.local.git_dir()
    }

    /// The remote's name.
    pub fn remote(&self) -> &str {
        &self.remote
    }

    /// The identity sluice reaches the remote as at its address now;
    /// `None` for the host's own credentials.
    pub fn identity(&self) -> Result<Option<String>> {
        Ok(self.speaker()?.map(|s| s.identity.name()))
    }

    /// Who a commit pushed to the remote at its address now names as its
    /// author and committer: the identity's author where sluice reaches
    /// the remote as one, else [`Author::sluice`] (record 0019 as amended
    /// 2026-11-18; rule core.43). The identity's failure to give one is
    /// this one's.
    pub fn author(&self) -> Result<Author> {
        match self.speaker()? {
            Some(s) => s.identity.author(&s.url),
            None => Ok(Author::sluice()),
        }
    }

    /// sluice's identity on the remote at its address as the host's
    /// configuration holds it now, read with no process (129): `None` for
    /// the host's own credentials, or a remote on no forge the identity is
    /// on.
    fn speaker(&self) -> Result<Option<Speaker<'_>>> {
        let Some((identity, helper)) = &self.identity else {
            return Ok(None);
        };
        let url = self.local.remote_url(&self.remote)?;
        Ok(identity.url(&url).map(|url| Speaker {
            identity: identity.as_ref(),
            url,
            helper,
        }))
    }

    /// git for the remote, a read, `git <args> <the remote> <refs>`: a
    /// fetch or a listing. With an identity, the user's credential helpers
    /// are cleared for this call alone and sluice's is named in their
    /// place, given the credentials in this call's environment; nothing is
    /// written to a git config, so no other git finds them.
    ///
    /// The read runs under the retry policy, tried again whatever failed
    /// but the remote's answer, on credentials made afresh after the
    /// remote refused them, since a forge may refuse ones it has just given
    /// (134, rule core.29). A refusal that stands has what each refused
    /// credential was added to git's words, for whoever reports the
    /// failure.
    pub(crate) fn remote_git(&self, args: &[&str], refs: &[&str]) -> Result<Output> {
        let called = self.call(Kind::Read, args, refs)?;
        Ok(match called.note {
            Some(note) => told(called.out, &note),
            None => called.out,
        })
    }

    /// git for the remote, a call of `kind`, `git <args> <the remote>
    /// <refs>`, under the retry policy: its last try's output, what each
    /// credential the remote refused was, apart from git's words
    /// (sluice's, never the remote's, 133), and how the last try stands
    /// when it failed. A try after a credential the remote refused carries
    /// one made afresh. The remote is reached where its address was when
    /// the call began, and the same arguments go every time, so a push
    /// keeps its target, its refspec and its lease (rule core.5).
    fn call(&self, kind: Kind, args: &[&str], refs: &[&str]) -> Result<Called> {
        let fetch = args.first() == Some(&"fetch");
        if fetch && self.under_own_fetch()? {
            tracing::debug!(
                "a fetch of sluice's own holds this repository's fetch lock above this process, \
                 so nothing is fetched and its refs are read as they stand"
            );
            return Ok(Called {
                out: fetched_nothing(),
                note: None,
                judged: None,
            });
        }
        let speaker = self.speaker()?;
        let mut line = args.to_vec();
        line.push(speaker.as_ref().map_or(&self.remote, |s| &s.url));
        line.extend(refs);
        let mut attempt = |line: &[&str], given: Option<&Credentials>| -> Result<Attempt> {
            // One fetch at a time in this repository, held for the try
            // alone, never over a pause between tries, and named to its
            // git, so a sluice its git runs waits on no lock its ancestor
            // holds (record 0110); and waited for no longer than a try may
            // take: one held past it is a try the remote never judged
            // (record 0109).
            let fetching = match fetch {
                true => match self.fetching()? {
                    Ok(held) => Some(held),
                    Err(held) => return Ok(Attempt::Held(held)),
                },
                false => None,
            };
            let held = fetching.as_ref().map(|(_, path)| path.as_path());
            let tried = self.git_as(speaker.as_ref(), given, line, held);
            drop(fetching);
            tried.map(Attempt::Ran)
        };
        calling(
            &self.policy,
            kind,
            &self.remote,
            speaker.as_ref(),
            &line,
            &mut attempt,
            &|s, out| self.unasked(s, out),
        )
    }

    /// What a call as `s` that the remote refused was refused on, where git
    /// never asked sluice's helper for `s`'s credentials: none, git getting
    /// none from the helper, or a credential git sent of its own in their
    /// place, named where sluice can find it, in the address git reached
    /// the remote by once its configuration rewrote the speaker's (134;
    /// record 0019 as amended 2026-11-18). The remote's own words, before
    /// it, name whose it was where the remote does, as GitHub's
    /// `Permission to <repository> denied to <account>` does.
    fn unasked(&self, s: &Speaker, out: &Output) -> String {
        let who = s.identity.name();
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("could not read Username") || err.contains("could not read Password") {
            return format!(
                "{} asked for credentials, and git got none from sluice's helper, `{}`, for {who}",
                self.remote,
                s.helper.trim_start_matches('!')
            );
        }
        // git's own reading of the address, with nothing sent to the remote.
        let reached = command(self.local.git_dir(), &["ls-remote", "--get-url", &s.url])
            .worker_output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| s.url.clone());
        // A header stored for the address a rewrite reached, which the
        // header emptied for the speaker's own address does not cover.
        let header = reached != s.url
            && command(
                self.local.git_dir(),
                &["config", "--get-urlmatch", "http.extraheader", &reached],
            )
            .worker_output()
            .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());
        format!(
            "{} refused {}, which git sent in place of {who}'s credentials, never asking sluice for them",
            self.remote,
            own_credential(
                &reached,
                header,
                std::env::var_os("HOME").as_deref().map(Path::new)
            )
        )
    }

    /// The lock that makes one fetch at a time of this repository's, by any
    /// process: two at once race to write the refs a fetch writes here,
    /// sluice's under `refs/sluice-fetched/` and the remote-tracking refs,
    /// and the one that loses would read its own lock's words as the
    /// remote's refusal (rules core.28, core.29). The lock held, and its
    /// path, which the fetch's git is told (record 0110). It is waited for
    /// as long as a try may take, and no longer: `Err` with the lock's
    /// path when another fetch held it all that time, which its own limit
    /// ends (record 0109).
    fn fetching(&self) -> Result<std::result::Result<(std::fs::File, PathBuf), PathBuf>> {
        let path = self.fetch_lock()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let until = Instant::now() + self.policy.try_for();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Ok((file, path))),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => {
                    return Err(e).with_context(|| format!("locking {}", path.display()))
                }
            }
            if Instant::now() >= until {
                return Ok(Err(path));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The repository's fetch lock, in its common git directory.
    fn fetch_lock(&self) -> Result<PathBuf> {
        Ok(self.local.common_dir()?.join(FETCH_LOCK))
    }

    /// Whether this process runs under a fetch of sluice's own of this
    /// repository, as the `reference-transaction` hook a fetch's git runs
    /// does, its packing of refs included: its environment names this
    /// repository's fetch lock, as sluice names it to the git of a fetch
    /// that holds it, and the lock is held. Such a process takes no fetch
    /// lock and fetches nothing, since the lock is its ancestor's and is
    /// let go only once it ends (record 0110).
    fn under_own_fetch(&self) -> Result<bool> {
        let Some(named) = std::env::var_os(FETCH_HELD_VAR) else {
            return Ok(false);
        };
        let path = self.fetch_lock()?;
        if std::fs::canonicalize(&named).ok().as_deref() != Some(path.as_path()) {
            return Ok(false);
        }
        let file = match std::fs::OpenOptions::new().write(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
        };
        match file.try_lock() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(std::fs::TryLockError::Error(e)) => {
                Err(e).with_context(|| format!("locking {}", path.display()))
            }
        }
    }

    /// Fetch `refspec`, whose source is `asked`, with `flags` before the
    /// remote. `Ok(false)` when the remote has no such ref; a fetch that
    /// fails otherwise is a [`ReadRefused`] (133).
    pub(crate) fn fetch_spec(&self, asked: &str, refspec: &str, flags: &[&str]) -> Result<bool> {
        self.fetch_specs(asked, &[refspec], flags)
    }

    /// [`PushedRefs::fetch_spec`] of every one of `refspecs`, in one fetch
    /// under the retry policy (rule core.29): `Ok(false)` when the remote
    /// has no ref one of them names exactly, which fetches none of them. A
    /// fetch the forge would not give sluice's identity a credential for is
    /// a [`ReadRefused`] in the forge's words, as one git was refused is
    /// (133; rule core.28).
    pub(crate) fn fetch_specs(
        &self,
        asked: &str,
        refspecs: &[&str],
        flags: &[&str],
    ) -> Result<bool> {
        let mut args = vec!["fetch", "--quiet", "--no-tags", "--no-write-fetch-head"];
        args.extend(flags);
        let called = self
            .call(Kind::Read, &args, refspecs)
            .map_err(|e| withheld_read(&self.remote, asked, e));
        let Called { out, note, .. } = called?;
        if out.status.success() {
            return Ok(true);
        }
        let said = String::from_utf8_lossy(&out.stderr);
        if said.contains(NO_SUCH_REF) {
            return Ok(false);
        }
        // A race lost to a fetch no lock held, a person's or an older
        // sluice's, as each try was: the remote refused nothing, and the
        // refs hold what that fetch brought.
        if lost_race(&said) {
            return Ok(true);
        }
        Err(ReadRefused {
            remote: self.remote.clone(),
            asked: asked.into(),
            said: said.trim_end().into(),
            note,
        }
        .into())
    }

    /// git for the remote, one try: with the host's own credentials when
    /// `given` is `None`, and otherwise with `speaker`'s helper alone, given
    /// them. A fetch holding the repository's fetch lock names it, `held`,
    /// to its git and so to every process its git runs (record 0110). It
    /// runs in a process group of its own for as long as a try may take:
    /// what git printed, and `true` where that limit came first and the
    /// group was ended, the remote's helper and any hook with it (134;
    /// record 0109).
    ///
    /// As `speaker`, git is given the speaker's credentials alone: the
    /// host's helpers are emptied, and the headers git's configuration
    /// holds for the speaker's address with them, as `actions/checkout`
    /// stores its job's token there, on the call's command line, the most
    /// specific match for the address and the last read (record 0019 as
    /// amended 2026-11-18). The try says whether git asked the speaker's
    /// helper for the credentials: one that did not sent the remote
    /// whatever git found itself, the helper marking a file the call made
    /// for it ([`ASKED_VAR`]).
    fn git_as(
        &self,
        speaker: Option<&Speaker>,
        given: Option<&Credentials>,
        args: &[&str],
        held: Option<&Path>,
    ) -> Result<Tried> {
        let (helper, headers);
        let mut asked = None;
        let mut cmd = match (speaker, given) {
            (Some(s), Some(c)) => {
                helper = format!("credential.helper={}", s.helper);
                headers = format!("http.{}.extraheader=", s.url);
                let mut all = vec!["-c", "credential.helper=", "-c", &helper, "-c", &headers];
                all.extend(args);
                let mut cmd = command(self.local.git_dir(), &all);
                let mark = Asked::make(&self.local.common_dir()?)?;
                cmd.env(USERNAME_VAR, &c.username)
                    .env(PASSWORD_VAR, &c.password)
                    .env(ASKED_VAR, &mark.path);
                asked = Some(mark);
                cmd
            }
            _ => command(self.local.git_dir(), args),
        };
        if let Some(lock) = held {
            cmd.env(FETCH_HELD_VAR, lock);
        }
        let ended = weir_bounded::output(&mut cmd, self.policy.try_for())
            .with_context(|| format!("running git {}", args.join(" ")))?;
        let out = Output {
            status: ended.status,
            stdout: ended.stdout,
            stderr: ended.stderr,
        };
        Ok(Tried {
            out,
            stopped: ended.stopped,
            asked: asked.is_some_and(|m| m.marked()),
        })
    }

    /// `git push <args> <the remote> <refspec>` under the retry policy,
    /// setting the remote's ref `name` to `to`, or removing it when `to` is
    /// `None`. A push the remote never judged is tried again with the same
    /// arguments, so the same refspec and lease (134; rules core.5,
    /// core.29). A push whose answer git did not read is never made again:
    /// `name` is read once, and the push is done when the remote holds
    /// `to`, or no `name` for a removal (122; ADR 0017).
    pub(crate) fn push(
        &self,
        args: &[&str],
        refspec: &str,
        name: &str,
        to: Option<&str>,
    ) -> Result<Pushed> {
        let called = self.call(Kind::Push, args, &[refspec])?;
        let out = match called.note {
            Some(note) => told(called.out, &note),
            None => called.out,
        };
        match called.judged {
            None => Ok(Pushed::Done),
            Some(Judged::Unknown) => {
                let shown = name.strip_prefix("refs/heads/").unwrap_or(name);
                let holds = match self.on_remote(name) {
                    Ok(at) if at.as_deref() == to => return Ok(Pushed::Done),
                    Ok(Some(at)) => format!("{} holds {shown} at {at}", self.remote),
                    Ok(None) => format!("{} has no {shown}", self.remote),
                    Err(e) => format!("{e:#}"),
                };
                Ok(Pushed::Unanswered(out, holds))
            }
            Some(_) => Ok(Pushed::Refused(out)),
        }
    }

    /// The revision the remote's ref `name` holds, listed under the retry
    /// policy as a read; `None` when it has no such ref.
    fn on_remote(&self, name: &str) -> Result<Option<String>> {
        let out = self.remote_git(&["ls-remote", "--refs"], &[name])?;
        if !out.status.success() {
            bail!(
                "listing {name} on {} failed: {}",
                self.remote,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .find(|(_, r)| *r == name)
            .map(|(at, _)| at.to_string()))
    }

    fn tracking(&self, branch: &str) -> String {
        format!("refs/remotes/{}/{branch}", self.remote)
    }

    /// Fetch `branch` into its remote-tracking ref; `None` when the remote
    /// has no such branch, a [`ReadRefused`] when it cannot be read. An
    /// intake mark an older sluice left on the remote is fetched by nothing
    /// (design D23).
    fn fetch(&self, branch: &str) -> Result<Option<Rev>> {
        let src = format!("refs/heads/{branch}");
        let spec = format!("+{src}:{}", self.tracking(branch));
        if !self.fetch_specs(&src, &[&spec], &[])? {
            return Ok(None);
        }
        self.local.read_ref(&self.tracking(branch))
    }

    /// Fetch the remote's ref `src` into the local ref `dst`, which it
    /// replaces; `None` when the remote has no such ref, a [`ReadRefused`]
    /// when it cannot be read. Writes no branch.
    pub fn fetch_ref(&self, src: &str, dst: &str) -> Result<Option<Rev>> {
        if !self.fetch_spec(src, &format!("+{src}:{dst}"), &[])? {
            return Ok(None);
        }
        self.local.read_ref(dst)
    }

    /// A local ref's revision, `None` when there is none.
    pub fn read_ref(&self, name: &str) -> Result<Option<Rev>> {
        self.local.read_ref(name)
    }

    /// The repository, as gix reads its refs and objects.
    pub fn local(&self) -> &LocalRefs {
        &self.local
    }

    /// Set the remote's branch `branch` to `rev`, whatever it held: a
    /// session's own branch, which no policy protects, pushed for its
    /// pull request.
    pub fn push_branch(&self, rev: &Rev, branch: &str) -> Result<()> {
        let name = format!("refs/heads/{branch}");
        let refspec = format!("+{rev}:{name}");
        let pushed = self.push(
            &["push", "--porcelain", "--no-verify"],
            &refspec,
            &name,
            Some(rev.as_str()),
        )?;
        if let Some(why) = pushed.why() {
            bail!("pushing {branch} to {} failed: {why}", self.remote);
        }
        Ok(())
    }
}

/// A read of `asked` on `remote` that failed with `e`: a [`ReadRefused`]
/// in the forge's words, with sluice's note beside them, where the forge
/// gave sluice's identity no credential for it (133; rule core.28), and
/// `e` otherwise.
fn withheld_read(remote: &str, asked: &str, e: anyhow::Error) -> anyhow::Error {
    let Some(w) = withheld(&e) else { return e };
    ReadRefused {
        remote: remote.into(),
        asked: asked.into(),
        said: w.said.clone(),
        note: w.note.clone(),
    }
    .into()
}

/// One try of a git call to the remote, as [`calling`] is told it.
enum Attempt {
    /// git ran: what it printed, whether its limit came first, and whether
    /// it asked sluice's helper for the speaker's credentials.
    Ran(Tried),
    /// Another fetch held the repository's fetch lock, at this path, for
    /// as long as a try may take: a try the remote never judged (record
    /// 0109).
    Held(PathBuf),
}

/// The tries of a git call to `remote` of `kind`, `line` its whole command
/// line, under `policy` (134; rule core.29): each try is `attempt` of the
/// same line, given the speaker's credentials where there is a speaker,
/// ones made afresh after the remote refused the last that git asked
/// sluice's helper for; `unasked` says what the remote refused where git
/// never asked for them. Its last try's output, what each credential the
/// remote refused was, apart from git's words (133), and how the last try
/// stands when it failed. A fault of sluice's own, git not run or no
/// credential given, is its error, and no try follows it.
fn calling(
    policy: &Policy,
    kind: Kind,
    remote: &str,
    speaker: Option<&Speaker>,
    line: &[&str],
    attempt: &mut dyn FnMut(&[&str], Option<&Credentials>) -> Result<Attempt>,
    unasked: &dyn Fn(&Speaker, &Output) -> String,
) -> Result<Called> {
    // Why sluice could not make a try: git not run, or no credential.
    let mut fault = None;
    let mut unrenewed = None;
    let mut refused: Vec<String> = Vec::new();
    // The password of the credentials the remote refused last, which a
    // renewal replaces.
    let mut refused_password = String::new();
    // What the remote refused where git never asked for the speaker's
    // credentials.
    let mut refused_unasked = None;
    let mut last = None;
    // Set while the last try found the fetch lock held past its limit.
    let mut waited = None;
    let ran = policy.run(kind, |t| {
        let given = match speaker {
            None => None,
            Some(s) if t.renew() => match s.identity.renew(&s.url, &refused_password) {
                Ok(c) => Some(c),
                Err(e) => {
                    unrenewed = Some(e);
                    return Err(Failed::new(Judged::Answered, ()));
                }
            },
            Some(s) => match s.identity.credentials(&s.url) {
                Ok(c) => Some(c),
                Err(e) => {
                    let e = e.context(format!("{} on {remote}", s.identity.name()));
                    fault = Some(e);
                    return Err(Failed::new(Judged::Answered, ()));
                }
            },
        };
        let tried = match attempt(line, given.as_ref()) {
            Ok(Attempt::Held(held)) => {
                waited = Some(held);
                return Err(Failed::new(Judged::Never, ()));
            }
            Ok(Attempt::Ran(tried)) => tried,
            Err(e) => {
                fault = Some(e);
                return Err(Failed::new(Judged::Answered, ()));
            }
        };
        waited = None;
        let (out, asked) = match tried {
            // A try git did not end within its limit: whether the remote
            // judged it is not known, so a read is tried again and a
            // push is settled by reading what the remote holds (122).
            Tried {
                out, stopped: true, ..
            } => {
                let verb = line.first().copied().unwrap_or("call");
                last = Some(told(
                    out,
                    &weir_bounded::stopped(&format!("git {verb}"), remote, policy.try_for()),
                ));
                return Err(Failed::new(Judged::Unknown, ()));
            }
            Tried { out, .. } if out.status.success() => return Ok(out),
            Tried { out, asked, .. } => (out, asked),
        };
        let mut judged = judged(&out, kind, speaker.is_some());
        match (judged, speaker, given) {
            (Judged::Credential, Some(_), Some(c)) if asked => {
                refused.push(c.about);
                refused_password = c.password;
            }
            // The remote refused what git sent of its own, or nothing
            // sluice gave: its answer, which no credential bought
            // afresh changes, since git sends the same again (134;
            // record 0019 as amended 2026-11-18).
            (Judged::Credential, Some(s), Some(_)) => {
                refused_unasked = Some(unasked(s, &out));
                judged = Judged::Answered;
            }
            _ => {}
        }
        last = Some(out);
        Err(Failed::new(judged, ()))
    });
    let stopped = match ran {
        Ok(out) => {
            return Ok(Called {
                out,
                note: None,
                judged: None,
            })
        }
        Err(stopped) => stopped,
    };
    if let Some(e) = fault {
        return Err(e);
    }
    if let Some(lock) = waited {
        bail!(
            "another fetch of this repository held {} past {}, the most a try waits for it",
            lock.display(),
            span(policy.try_for())
        );
    }
    let out = last.expect("a try git failed");
    let note = match (unrenewed, refused_unasked) {
        (Some(e), _) => Some(format!(
            "{remote} refused {}, and none could be made afresh: {e:#}",
            refused.join(", then ")
        )),
        (None, Some(unasked)) if refused.is_empty() => Some(unasked),
        (None, Some(unasked)) => Some(format!(
            "{remote} refused {}; {unasked}",
            refused.join(", then ")
        )),
        (None, None) if stopped.judged == Judged::Credential && !refused.is_empty() => {
            Some(format!("{remote} refused {}", refused.join(", then ")))
        }
        (None, None) => None,
    };
    Ok(Called {
        out,
        note,
        judged: Some(stopped.judged),
    })
}

/// A git call to the remote under the retry policy: its last try's output,
/// what each credential the remote refused was, and how the last try stood
/// when it failed.
struct Called {
    out: Output,
    note: Option<String>,
    judged: Option<Judged>,
}

/// One try of a git call to the remote ([`PushedRefs::git_as`]): what git
/// printed, whether its limit came first, and whether git asked sluice's
/// helper for the speaker's credentials.
struct Tried {
    out: Output,
    stopped: bool,
    asked: bool,
}

/// The file a git call as sluice's identity names to its helper in
/// [`ASKED_VAR`], which the helper marks once it gives git the
/// credentials: made empty, for this user alone, in the repository's
/// common git directory, and removed with the try.
struct Asked {
    path: PathBuf,
}

impl Asked {
    /// A file of its own in `dir`, made afresh.
    fn make(dir: &Path) -> Result<Asked> {
        use std::os::unix::fs::OpenOptionsExt as _;
        use std::sync::atomic::{AtomicU64, Ordering};
        static MADE: AtomicU64 = AtomicU64::new(0);
        loop {
            let n = MADE.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("sluice-asked-{}-{n}", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(_) => return Ok(Asked { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).with_context(|| format!("making {}", path.display())),
            }
        }
    }

    /// Whether the helper marked it.
    fn marked(&self) -> bool {
        std::fs::metadata(&self.path).is_ok_and(|m| m.len() > 0)
    }
}

impl Drop for Asked {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// How a push of [`PushedRefs::push`] ended.
pub(crate) enum Pushed {
    /// The remote took it, or holds what it set though its answer was
    /// lost.
    Done,
    /// The remote answered it with a refusal, or it failed before it was
    /// sent: git's output, with what each refused credential was.
    Refused(Output),
    /// Its answer was not read, and the remote does not hold what it set:
    /// git's output, and what the remote holds.
    Unanswered(Output, String),
}

impl Pushed {
    /// Why the push was not done, for a report; `None` when it was.
    pub(crate) fn why(&self) -> Option<String> {
        match self {
            Pushed::Done => None,
            Pushed::Refused(out) => Some(refusal(out)),
            Pushed::Unanswered(out, holds) => Some(format!(
                "{}; the push went unanswered, and {holds}",
                refusal(out)
            )),
        }
    }
}

/// How a failed git call to the remote stands (134): answered, when git
/// printed the remote's answer for a ref or the remote has no such ref; a
/// credential refused before the remote answered for any ref; not known,
/// when the call may have been sent with its answer lost; never judged,
/// when no connection was made or the remote could not answer git's
/// request for its refs, before anything of a push was sent; and not
/// known for any wording sluice does not know, which is never pushed
/// again. On the host's own credentials, which sluice cannot make afresh
/// (`renewable` false), a refused push is answered.
fn judged(out: &Output, kind: Kind, renewable: bool) -> Judged {
    let err = String::from_utf8_lossy(&out.stderr);
    if kind == Kind::Read && lost_race(&err) {
        Judged::Never
    } else if answered(out) || ANSWERED.iter().any(|w| err.contains(w)) {
        Judged::Answered
    } else if refused(out) {
        match renewable || kind == Kind::Read {
            true => Judged::Credential,
            false => Judged::Answered,
        }
    } else if err.contains(UNANSWERED) {
        Judged::Unknown
    } else if NEVER.iter().any(|w| err.contains(w)) || unable(&err) {
        Judged::Never
    } else {
        Judged::Unknown
    }
}

/// The credential git sends the remote at `url` of its own, without
/// asking a helper, where sluice can name it: the user name and password
/// the address it reached the remote by carries, the header git's
/// configuration stores for that address where `header` says one is, or
/// the entry `.netrc` in `home` holds for its host, which git's HTTP
/// library reads; never the secret itself.
fn own_credential(url: &str, header: bool, home: Option<&Path>) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or_default();
    let (userinfo, at) = match authority.rsplit_once('@') {
        Some((userinfo, at)) => (Some(userinfo), at),
        None => (None, authority),
    };
    if let Some(userinfo) = userinfo {
        let user = userinfo.split(':').next().unwrap_or_default();
        return format!("the user name {user:?} and the password in the address git reached it by");
    }
    if header {
        let scheme = url.split_once("://").map_or("", |(scheme, _)| scheme);
        return format!("the header git's configuration stores for {scheme}://{rest}");
    }
    let host = match at.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => at,
    };
    if let Some(netrc) = home.map(|h| h.join(".netrc")) {
        if std::fs::read_to_string(&netrc).is_ok_and(|text| netrc_names(&text, host)) {
            return format!("the credential {} holds for {host}", netrc.display());
        }
    }
    format!("a credential of git's own for {host} that sluice cannot name")
}

/// Whether a `.netrc`'s `text` gives a credential for `host`: an entry
/// for that machine, or a default one.
fn netrc_names(text: &str, host: &str) -> bool {
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        match word {
            "default" => return true,
            "machine" if words.next() == Some(host) => return true,
            _ => {}
        }
    }
    false
}

/// What git prints when the remote has no ref a fetch asks for.
const NO_SUCH_REF: &str = "couldn't find remote ref";

/// The file in the repository's git directory whose lock one fetch holds
/// at a time ([`PushedRefs::fetching`]).
const FETCH_LOCK: &str = "sluice-fetch.lock";

/// The variable a fetch holding the repository's fetch lock names its path
/// in, to its git and every process that runs under it
/// ([`PushedRefs::under_own_fetch`]).
const FETCH_HELD_VAR: &str = "SLUICE_FETCH_HELD";

/// What a fetch reads as where it fetched nothing under a fetch of
/// sluice's own of the repository: done, the refs as that fetch leaves
/// them, as after a race lost to it (record 0086).
fn fetched_nothing() -> Output {
    use std::os::unix::process::ExitStatusExt as _;
    Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Whether git says a fetch could not write a ref of this repository's,
/// as when another fetch wrote it first: `cannot lock ref '<name>': is at
/// <a> but expected <b>`, or its lock file standing. A local race, which
/// the remote never judged: nothing of a remote's is a local ref's lock.
fn lost_race(err: &str) -> bool {
    err.lines().any(|l| {
        let l = l
            .trim_start_matches("error: ")
            .trim_start_matches("fatal: ");
        l.starts_with("cannot lock ref '")
            || l.starts_with("Unable to create '") && l.contains(".lock': File exists")
    })
}

/// What git prints for a call the remote answered with no line for a ref:
/// a fetch's ref the remote lacks, and a removal of one it lacks, which
/// git finds in the remote's listing.
const ANSWERED: &[&str] = &[NO_SUCH_REF, "remote ref does not exist"];

/// What git prints when a request it sent the remote over HTTP failed,
/// git's `remote-curl.c`: after a push was sent, so its answer was lost.
const UNANSWERED: &str = "RPC failed";

/// What git and curl print when no connection to the remote was made.
const NEVER: &[&str] = &[
    "Could not resolve host",
    "Could not resolve hostname",
    "Failed to connect",
    "Connection refused",
    "Connection timed out",
    "Operation timed out",
];

/// Whether git says the remote answered its request for the refs with a
/// 5xx or a 429: unable to answer then, and nothing of a push sent.
fn unable(err: &str) -> bool {
    const SAID: &str = "The requested URL returned error: ";
    err.match_indices(SAID).any(|(at, _)| {
        let code = &err[at + SAID.len()..];
        code.starts_with('5') || code.starts_with("429")
    })
}

/// What git prints when the remote refuses a call's credentials over
/// HTTP: 404, which GitHub answers a token that may not read the
/// repository, 401 and 403.
const REFUSED: &[&str] = &[
    // repository '…' not found; GitHub's own: Repository not found.
    "not found",
    // 401
    "Authentication failed",
    // 401, on the host's own credentials, when it has none to give
    "could not read Username",
    "could not read Password",
    // The requested URL returned error: …
    "error: 401",
    "error: 403",
    "error: 404",
    // RPC failed; HTTP …
    "HTTP 401",
    "HTTP 403",
    "HTTP 404",
];

/// Whether git says the remote refused the call's credentials.
fn refused(out: &Output) -> bool {
    let err = String::from_utf8_lossy(&out.stderr);
    REFUSED.iter().any(|w| err.contains(w))
}

/// Whether git printed the remote's answer for a ref: a line of `git push
/// --porcelain`, `<flag>\t<from>:<to>\t<summary>`. No other output of a
/// failed git call of sluice's holds a tab.
fn answered(out: &Output) -> bool {
    out.stdout.contains(&b'\t')
}

/// `out` with `said` on the line after what git printed on stderr, or as
/// all of it where git printed nothing.
fn told(mut out: Output, said: &str) -> Output {
    out.stderr.truncate(out.stderr.trim_ascii_end().len());
    if !out.stderr.is_empty() {
        out.stderr.push(b'\n');
    }
    out.stderr.extend_from_slice(said.as_bytes());
    out
}

/// 152, rule core.5, record 0124 (design D10): a leased push of a branch
/// from `expected` to `to`, `None` for a removal, that the remote answered
/// for the ref with a refusal, a hook's, a ruleset's or push protection's,
/// read against the branch as fetched after it, `found`: holding `to`, it
/// moved; still where the move expected it, the forge refused the move, in
/// its words whole; elsewhere, it was moved meanwhile.
fn answered_refusal(found: Option<Rev>, expected: &str, to: Option<&Rev>, out: &Output) -> Moved {
    match found {
        Some(f) if Some(&f) == to => Moved::Moved,
        None if to.is_none() => Moved::Moved,
        f if f.as_ref().map_or("", Rev::as_str) == expected => Moved::Refused(refusal(out)),
        f => Moved::Elsewhere(f),
    }
}

/// Why a `git push --porcelain` failed: the remote's reason for each ref
/// it refused, which porcelain prints on stdout, then what git printed on
/// stderr, which alone says only that some refs failed.
pub(crate) fn refusal(out: &Output) -> String {
    let report = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    report
        .lines()
        .filter(|l| l.starts_with('!'))
        .filter_map(|l| l.rsplit('\t').next())
        .map(str::trim)
        .chain(std::iter::once(err.trim()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn command(git_dir: &Path, args: &[&str]) -> Command {
    let mut c = crate::workers::command("git", "git");
    c.arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove(USERNAME_VAR)
        .env_remove(PASSWORD_VAR)
        .env_remove(ASKED_VAR)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .stdin(Stdio::null());
    c
}

/// The URL of the remote `remote` of the repository holding `path`, as
/// configured, before any `insteadOf`, read with no process (129).
pub fn remote_url(path: &Path, remote: &str) -> Result<String> {
    LocalRefs::open(path)?.remote_url(remote)
}

/// `path` quoted for the shell git runs a `!` helper with.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

impl Branch for PushedRefs {
    /// 42: the remote's revision of the branch, fetched on every read.
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        self.fetch(branch)
    }

    /// 129: the remote's tracking ref of the branch as last fetched, with
    /// no fetch.
    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        self.local.read_ref(&self.tracking(branch))
    }

    /// Every object fetched is on this host.
    fn holds(&self, rev: &Rev) -> Result<bool> {
        self.local.holds(rev)
    }

    /// 124: the host's `refs/heads/<branch>`, sluice's copy of the remote's
    /// branch, read with no fetch.
    fn copy(&self, branch: &str) -> Result<HostCopy> {
        Ok(HostCopy::At(self.local.read(branch)?))
    }

    /// 124: set the copy with gix, from exactly the value expected.
    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        self.local.level_copy(branch, expected, to)
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        self.local.read_file(rev, path)
    }

    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        self.local.form(onto, head)
    }

    /// The landing written as whoever pushes it, [`PushedRefs::author`]:
    /// nothing is written where that cannot be read.
    fn commit(&self, _: &str, commit: &LandingCommit) -> Result<Rev> {
        let author = self.author()?;
        self.local.commit_as(commit, &author)
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

    /// 45, 46: push `to`, accepted only while the remote's branch is
    /// `expected`. A push the remote never judged, its credential refused
    /// among them, is tried again under the retry policy with the same
    /// lease (134); a refusal at the ref is reported with what the remote
    /// holds, never retried blind (rules core.5, core.29); any other
    /// failure is an error, which leaves the landing for the next step to
    /// settle. An accepted push is the move, whatever the host's copy does
    /// next (122).
    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        self.leased(branch, expected.as_str(), Some(to))
    }

    /// branches-1 104: push `at` as the branch, leased on its being absent
    /// (`--force-with-lease=<ref>:` with nothing after the colon).
    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        self.leased(branch, "", Some(at))
    }

    /// branches-1 105: delete the remote's branch, leased on `expected`;
    /// once it is gone, so are its local branch and remote-tracking ref.
    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        let moved = self.leased(branch, expected.as_str(), None)?;
        if moved == Moved::Moved {
            self.local.delete_ref(&format!("refs/heads/{branch}"))?;
            self.local.delete_ref(&self.tracking(branch))?;
        }
        Ok(moved)
    }
}

impl PushedRefs {
    /// Set the branch to `to`, or remove it when `to` is `None`, accepted
    /// only while the remote's branch is `expected`, or absent when
    /// `expected` is empty. A push the remote never judged, its credential
    /// refused among them, is tried again under the retry policy with the
    /// same lease (134); a refusal at the ref is reported with what the
    /// remote holds, never retried blind (rules core.5, core.29). A push
    /// whose answer git never read is never made again: the branch is
    /// read, and the move is made when the remote holds `to` (122);
    /// otherwise, and on any other failure, it is an error, which leaves a
    /// landing for the next step to settle.
    fn leased(&self, branch: &str, expected: &str, to: Option<&Rev>) -> Result<Moved> {
        let name = format!("refs/heads/{branch}");
        let lease = format!("--force-with-lease={name}:{expected}");
        let refspec = format!("{}:{name}", to.map_or("", Rev::as_str));
        let pushed = self.push(
            &["push", "--porcelain", "--no-verify", &lease],
            &refspec,
            &name,
            to.map(Rev::as_str),
        )?;
        if let Pushed::Refused(out) = &pushed {
            let report = String::from_utf8_lossy(&out.stdout);
            let refused_at_the_ref = report
                .lines()
                .any(|l| l.starts_with('!') && l.contains("[rejected]"));
            if refused_at_the_ref {
                return Ok(Moved::Elsewhere(self.fetch(branch)?));
            }
            // 152, rule core.5, record 0124 (design D10): a refusal the
            // remote answered for the ref, `[remote rejected]` by a hook, a
            // ruleset or push protection, is read against the branch,
            // fetched: still where the move expected it, the forge refused
            // the move, in its words whole; holding `to`, it moved;
            // elsewhere, it was moved meanwhile.
            let answered_for_the_ref = report.lines().any(|l| l.starts_with('!'));
            if answered_for_the_ref {
                return Ok(answered_refusal(self.fetch(branch)?, expected, to, out));
            }
        }
        match pushed.why() {
            None => Ok(Moved::Moved),
            Some(why) => bail!("pushing {branch} to {} failed: {why}", self.remote),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{answered_refusal, lost_race, netrc_names, own_credential, Moved, Output, Rev};

    /// Record 0019 as amended 2026-11-18: a credential git sent of its own
    /// is named by where git found it, the address's user name, a header
    /// stored for the address or the user's `.netrc`, and never by its
    /// secret.
    #[test]
    fn a_credential_git_sends_of_its_own_is_named_by_where_it_is() {
        assert_eq!(
            own_credential("https://job:secret@github.com/o/r.git", true, None),
            "the user name \"job\" and the password in the address git reached it by"
        );
        let home = std::env::temp_dir().join(format!("sluice-netrc-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let named = own_credential("https://github.com:443/o/r.git", false, Some(&home));
        std::fs::write(
            home.join(".netrc"),
            "machine gitlab.com login a password b\nmachine github.com\n  login x-access-token\n  password secret\n",
        )
        .unwrap();
        let netrc = own_credential("https://github.com/o/r.git", false, Some(&home));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(
            named,
            "a credential of git's own for github.com that sluice cannot name"
        );
        assert_eq!(
            netrc,
            format!(
                "the credential {} holds for github.com",
                home.join(".netrc").display()
            )
        );
        assert!(!netrc.contains("secret"));
        assert_eq!(
            own_credential("https://mirror.example/o/r.git", true, Some(&home)),
            "the header git's configuration stores for https://mirror.example/o/r.git"
        );
        assert!(netrc_names("default login a password b", "github.com"));
        assert!(!netrc_names(
            "machine gitlab.com login github.com",
            "github.com"
        ));
    }

    /// Rules core.28, core.29: a fetch that lost a race for a ref of this
    /// repository's is told from a remote's refusal by git's own words.
    #[test]
    fn a_lost_race_is_told_by_gits_words() {
        // deneb, 2026-11-06, the runner's copy of swancloud.
        assert!(lost_race(
            "error: cannot lock ref 'refs/sluice-fetched/origin/intake/main': is at \
             08a038f7b4c564805aea19d394784850e6a76580 but expected \
             36bceb3675e9d6a79ec9ab94f67ec63111c22fcc"
        ));
        assert!(lost_race(
            "error: cannot lock ref 'refs/remotes/origin/main': Unable to create \
             '/r/.git/refs/remotes/origin/main.lock': File exists."
        ));
        assert!(!lost_race(
            "remote: Repository not found.\nfatal: repository 'https://github.com/o/r/' not found"
        ));
        assert!(!lost_race(
            "fatal: unable to access 'https://github.com/o/r/': Could not resolve host"
        ));
    }

    /// git's output of a push the remote refused for its ref, its
    /// porcelain line on stdout and git's words on stderr.
    fn declined() -> Output {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(256),
            stdout: b"To /remote.git\n!\trefs/heads/main:refs/heads/main\t[remote rejected] (pre-receive hook declined)\nDone\n".to_vec(),
            stderr: b"remote: push declined due to repository rule violations\nerror: failed to push some refs to '/remote.git'\n".to_vec(),
        }
    }

    /// 152, rule core.5, record 0124 (design D10): a refusal the remote
    /// answered for the ref is read against the branch fetched after it:
    /// still where the move expected it, the forge refused the move, its
    /// porcelain reason and its words both said; holding the landing, it
    /// moved; a removal it holds no branch for, done; anywhere else, moved
    /// meanwhile, with what it holds
    /// (`a_refused_push_with_the_branch_moved_reads_as_elsewhere`).
    #[test]
    fn a_refusal_for_the_ref_is_read_against_the_branch() {
        let (base, to, side) = (Rev::new("b0"), Rev::new("c1"), Rev::new("s2"));
        match answered_refusal(Some(base.clone()), "b0", Some(&to), &declined()) {
            Moved::Refused(words) => {
                assert!(
                    words.contains("[remote rejected] (pre-receive hook declined)"),
                    "{words}"
                );
                assert!(
                    words.contains("push declined due to repository rule violations"),
                    "{words}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            answered_refusal(Some(to.clone()), "b0", Some(&to), &declined()),
            Moved::Moved
        );
        assert_eq!(
            answered_refusal(None, "b0", None, &declined()),
            Moved::Moved
        );
        assert_eq!(
            answered_refusal(Some(side.clone()), "b0", Some(&to), &declined()),
            Moved::Elsewhere(Some(side))
        );
        assert_eq!(
            answered_refusal(None, "b0", Some(&to), &declined()),
            Moved::Elsewhere(None)
        );
    }
}
