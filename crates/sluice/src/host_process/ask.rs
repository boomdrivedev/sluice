//! What a command asks of sluice's host process (design D2, D14), as one
//! interface: the binding asks the host process through its socket, and
//! the tests' host answers in the test's own process (design D14).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Result};
use sluice_core::{Clock, Sluice};
use sluice_seams::{Part, RunKind};

use super::client::{Client, Connection};
use super::pass::Report;
use super::protocol::{Answer, FollowKind, Named, Reading, RoomView};
use super::room::Why;
use crate::{exec, PullRequests};

/// What an ask is made from: the command's binding of its repository, in
/// which the tests' host runs what it is asked.
pub struct Here<'h, 's> {
    pub s: &'h Sluice<'s>,
    pub host: &'h dyn exec::Host,
    pub pulls: &'h dyn PullRequests,
    pub clock: &'h dyn Clock,
}

impl Here<'_, '_> {
    /// The repository, as its common git directory.
    pub fn repository(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(self.host.repository()?))
    }
}

/// What a held room's asker is told next.
pub enum Holds {
    /// It waits, for this.
    Waiting(Why),
    /// The room is its: the part its run starts with.
    Held(Part),
}

/// A follow of a repository's passes, while it lives (D7).
pub trait Following {
    /// A pass's report, waiting up to `wait` for one; `None` where none
    /// came. An error where the host process ended under it.
    fn next(&mut self, wait: Duration) -> Result<Option<Report>>;
}

/// A session's or a preview's ask for room, while it lives (D12).
pub trait Holding {
    /// What it is told next, waiting up to `wait`; `None` where nothing
    /// changed. An error where the host process ended under it.
    fn next(&mut self, wait: Duration) -> Result<Option<Holds>>;
}

/// The requests of design D2, as a command asks them.
pub trait HostProcess {
    /// Whether the host process passes only when asked (D14): a follower
    /// asks a pass at each of its ticks.
    fn asked_only(&self) -> bool;

    /// List the repository in the host's index where it is not there and
    /// has sluice (135; D4).
    fn note(&self, here: &Here<'_, '_>) -> Result<()>;

    /// The repository has work: the host process, started where none
    /// answers, holds it and passes it at once.
    fn serve(&self, here: &Here<'_, '_>) -> Result<()>;

    /// The host process, started where none answers, which holds every
    /// repository in its index with work in play (135; D15).
    fn reach(&self, here: &Here<'_, '_>) -> Result<()>;

    /// One round of `branch` (D10): `{document, said, line}`.
    fn step(&self, here: &Here<'_, '_>, branch: &str) -> Result<serde_json::Value>;

    /// A pass of the repository now, once it ends (D14).
    fn pass(&self, here: &Here<'_, '_>) -> Result<Report>;

    /// A reading of the repository, once one is current, the host process
    /// started where none answers (D16).
    fn read(&self, here: &Here<'_, '_>) -> Result<Reading>;

    /// Follow the repository's passes as `kind`.
    fn follow<'a>(
        &'a self,
        here: &Here<'a, '_>,
        kind: FollowKind,
    ) -> Result<Box<dyn Following + 'a>>;

    /// Ask for a session's or a preview's room for the place `place`.
    fn hold<'a>(
        &'a self,
        here: &Here<'a, '_>,
        kind: RunKind,
        place: &str,
    ) -> Result<Box<dyn Holding + 'a>>;

    /// The process that answers now and its room; `None` where none
    /// answers, and nothing is started.
    fn room(&self, here: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>>;
}

/// The host process reached through its socket: the binding's.
#[derive(Default)]
pub struct Socketed {
    client: std::cell::OnceCell<Result<Client, String>>,
    /// Whether the process last followed passes only when asked, as its
    /// `hello` said (design D14).
    mode: std::cell::Cell<Option<bool>>,
}

impl Socketed {
    fn client(&self) -> Result<&Client> {
        self.client
            .get_or_init(|| Client::host().map_err(|e| format!("{e:#}")))
            .as_ref()
            .map_err(|e| anyhow!("{e}"))
    }
}

impl HostProcess for Socketed {
    /// As the process this command last followed said of itself, else as
    /// this command's own setting says (design D14).
    fn asked_only(&self) -> bool {
        self.mode.get().unwrap_or_else(super::asked_only)
    }

    fn note(&self, here: &Here<'_, '_>) -> Result<()> {
        let repository = here.repository()?;
        let index = self.client()?.paths().index();
        if index.list()?.contains(&repository) {
            return Ok(());
        }
        if has_sluice(here)? {
            index.note(&repository)?;
        }
        Ok(())
    }

    fn serve(&self, here: &Here<'_, '_>) -> Result<()> {
        self.client()?.serve(&here.repository()?)
    }

    fn reach(&self, _: &Here<'_, '_>) -> Result<()> {
        self.client()?.reach().map(drop)
    }

    fn step(&self, here: &Here<'_, '_>, branch: &str) -> Result<serde_json::Value> {
        self.client()?.step(&here.repository()?, branch)
    }

    fn pass(&self, here: &Here<'_, '_>) -> Result<Report> {
        self.client()?.pass(&here.repository()?)
    }

    fn read(&self, here: &Here<'_, '_>) -> Result<Reading> {
        self.client()?.read(&here.repository()?)
    }

    fn follow<'a>(
        &'a self,
        here: &Here<'a, '_>,
        kind: FollowKind,
    ) -> Result<Box<dyn Following + 'a>> {
        let c = self.client()?.follow(&here.repository()?, kind)?;
        self.mode.set(Some(c.named.asked_only));
        Ok(Box::new(Followed(c)))
    }

    fn hold<'a>(
        &'a self,
        here: &Here<'a, '_>,
        kind: RunKind,
        place: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        let c = self.client()?.hold(kind, &here.repository()?, place)?;
        Ok(Box::new(Asking {
            connection: Some(c),
            kind,
        }))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        self.client()?.room()
    }
}

/// Whether the repository `here` binds has sluice, records or a policy at a
/// revision the host holds, read with no process started (129).
pub fn has_sluice(here: &Here<'_, '_>) -> Result<bool> {
    if !here.s.seams().state.branches()?.is_empty() {
        return Ok(true);
    }
    let tips = here.host.tips()?;
    here.host.holds_policy(&tips)
}

struct Followed(Connection);

impl Following for Followed {
    fn next(&mut self, wait: Duration) -> Result<Option<Report>> {
        match self.0.next(Some(wait))? {
            Some(Answer::Pass(r)) => Ok(Some(r)),
            Some(Answer::Error(e)) => Err(anyhow!("{e}")),
            Some(_) | None => Ok(None),
        }
    }
}

struct Asking {
    connection: Option<Connection>,
    kind: RunKind,
}

impl Holding for Asking {
    fn next(&mut self, wait: Duration) -> Result<Option<Holds>> {
        let Some(c) = self.connection.as_mut() else {
            return Err(anyhow!("the room was held already"));
        };
        match c.next(Some(wait))? {
            Some(Answer::Waiting(why)) => Ok(Some(Holds::Waiting(why))),
            Some(Answer::Held) => {
                let c = self.connection.take().expect("asked above");
                Ok(Some(Holds::Held(Part::new(self.kind, Lent(c)))))
            }
            Some(Answer::Error(e)) => Err(anyhow!("{e}")),
            Some(_) | None => Ok(None),
        }
    }
}

/// A part of the host process's room, held over its connection until the
/// run begins (D12).
pub struct Lent(pub Connection);

impl Lent {
    /// The run began in `place`, in `run_dir`, its supervisor holding it.
    pub fn started(mut self, place: &str, run_dir: &Path) -> Result<()> {
        self.0.started(place, run_dir)
    }
}

/// Tell the host process the repository `here` binds has work, starting
/// it where none answers (135; design D15). One that cannot be reached is
/// said on stderr, and the command's own work stands: the next command of
/// sluice's that finds work in play starts it.
pub fn serve(here: &Here<'_, '_>) {
    if let Err(e) = here.host.host_process().serve(here) {
        crate::telemetry::warn(&format!(
            "sluice: warning: sluice's host process could not be reached, so nothing moves the queue until a command of sluice's starts it: {e:#}"
        ));
    }
}

/// List the repository `here` binds in the host's index where it has
/// sluice (135; design D4). One that cannot be listed is passed over.
pub fn note(here: &Here<'_, '_>) {
    let _ = here.host.host_process().note(here);
}

/// 135 (design D14): a command that moves no queue itself, `sluice
/// status`, `sluice overview` or the home view, reaches the host process
/// where one answers; where none answers, it reads whether anything is in
/// play as an idle tick reads it, with no process started, and starts one
/// only where something is, so a host process that crashed or slept is
/// started again by the next command of sluice's. What it could not reach
/// is said on stderr, and the command's answer stands. The process that
/// answers, with its room, where one does.
pub fn reach_if_in_play(here: &Here<'_, '_>) -> Option<(Named, RoomView)> {
    note(here);
    let process = here.host.host_process();
    match process.room(here) {
        Ok(Some(answered)) => return Some(answered),
        Ok(None) => {}
        Err(e) => {
            crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
            return None;
        }
    }
    if !super::pass::in_play(here.s) {
        return None;
    }
    if let Err(e) = process.reach(here) {
        crate::telemetry::warn(&format!(
            "sluice: warning: sluice's host process could not be started, so nothing moves the queue until a command of sluice's starts it: {e:#}"
        ));
        return None;
    }
    process.room(here).ok().flatten()
}

/// A watch's, a wait's or `sluice run`'s part in following the host
/// process (129; design D7, D9, D11): it asks a pass at each of its ticks
/// where passes are taken only when asked, keeps a follow open otherwise,
/// and reaches the process again at the tick after it ended, starting it
/// where none answers.
pub struct Follower<'a> {
    process: &'a dyn HostProcess,
    kind: FollowKind,
    following: Option<Box<dyn Following + 'a>>,
}

impl<'a> Follower<'a> {
    pub fn new(here: &Here<'a, '_>, kind: FollowKind) -> Follower<'a> {
        let mut f = Follower {
            process: here.host.host_process(),
            kind,
            following: None,
        };
        f.reach(here);
        f
    }

    /// Follow again where it follows nothing: the process ended under it,
    /// or could not be reached at its last tick.
    fn reach(&mut self, here: &Here<'a, '_>) {
        if self.following.is_some() {
            return;
        }
        match self.process.follow(here, self.kind) {
            Ok(f) => self.following = Some(f),
            Err(e) => crate::telemetry::warn(&format!(
                "sluice: warning: sluice's host process could not be reached; tried again at the next tick: {e:#}"
            )),
        }
    }

    /// One tick: a pass asked first where passes are taken only when
    /// asked; the reports of the passes since the last tick.
    pub fn tick(&mut self, here: &Here<'a, '_>) -> Vec<Report> {
        let mut reports = Vec::new();
        if self.process.asked_only() {
            match self.process.pass(here) {
                Ok(r) => reports.push(r),
                Err(e) => crate::telemetry::warn(&format!("sluice: warning: {e:#}")),
            }
        }
        self.reach(here);
        if let Some(f) = self.following.as_mut() {
            loop {
                match f.next(Duration::from_millis(1)) {
                    Ok(Some(r)) => reports.push(r),
                    Ok(None) => break,
                    Err(_) => {
                        self.following = None;
                        break;
                    }
                }
            }
        }
        reports
    }

    /// The next pass's report, waiting up to `wait` for one, a pass asked
    /// where passes are taken only when asked: as `sluice run` prints each.
    pub fn next(&mut self, here: &Here<'a, '_>, wait: Duration) -> Option<Report> {
        if self.process.asked_only() {
            return match self.process.pass(here) {
                Ok(r) => Some(r),
                Err(e) => {
                    crate::telemetry::warn(&format!("sluice: warning: {e:#}"));
                    None
                }
            };
        }
        self.reach(here);
        let f = self.following.as_mut()?;
        match f.next(wait) {
            Ok(r) => r,
            Err(_) => {
                self.following = None;
                None
            }
        }
    }
}

/// The host process as its own repositories' tasks ask it: its room, read
/// in place, and nothing else, since nothing in it asks its own socket.
pub struct Inside {
    pub room: super::room::Shared,
    pub named: Named,
}

impl HostProcess for Inside {
    fn asked_only(&self) -> bool {
        super::asked_only()
    }

    fn note(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn serve(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn reach(&self, _: &Here<'_, '_>) -> Result<()> {
        Ok(())
    }

    fn step(&self, _: &Here<'_, '_>, _: &str) -> Result<serde_json::Value> {
        Err(anyhow!(
            "a step is asked of the host process, not from within it"
        ))
    }

    fn pass(&self, _: &Here<'_, '_>) -> Result<Report> {
        Err(anyhow!(
            "a pass is asked of the host process, not from within it"
        ))
    }

    fn read(&self, _: &Here<'_, '_>) -> Result<Reading> {
        Err(anyhow!(
            "a reading is asked of the host process, not from within it"
        ))
    }

    fn follow<'a>(&'a self, _: &Here<'a, '_>, _: FollowKind) -> Result<Box<dyn Following + 'a>> {
        Err(anyhow!("the host process follows nothing of its own"))
    }

    fn hold<'a>(&'a self, _: &Here<'a, '_>, _: RunKind, _: &str) -> Result<Box<dyn Holding + 'a>> {
        Err(anyhow!(
            "a session's room is asked of the host process, not from within it"
        ))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(Some((self.named.clone(), super::view_of(&self.room)?)))
    }
}
