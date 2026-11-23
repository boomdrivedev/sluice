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
use super::protocol::{Added, Named, Reading, RoomView, SubscriptionAsk, SubscriptionLine};
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

/// What a session's or a preview's wait for a slot is told next.
pub enum Holds {
    /// It waits, for this.
    Waiting(Why),
    /// The room is its: the part its run starts with.
    Held(Part),
}

/// A subscription to the host process, while it lives (the change
/// `sluice-host-answers-every-read`, design D6 to D8).
pub trait Subscription {
    /// Whether the host process passes only when asked: its follower asks
    /// a pass at each of its ticks (D14).
    fn asked_only(&self) -> bool;
    /// Send an `add` or a `remove` line.
    fn say(&mut self, ask: &SubscriptionAsk) -> Result<()>;
    /// The next line, waiting up to `wait`; `None` where none came. An
    /// error where the host process ended under it.
    fn next(&mut self, wait: Duration) -> Result<Option<SubscriptionLine>>;
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

    /// A reading of the repository, once one is current, or with `afresh`
    /// once one begun after the ask ends, the host process started where
    /// none answers (D16).
    fn read(&self, here: &Here<'_, '_>, afresh: bool) -> Result<Reading>;

    /// Ask for a session's or a preview's room for the place `place`, held
    /// for `branch`, the branch its run is held to (154).
    fn hold<'a>(
        &'a self,
        here: &Here<'a, '_>,
        kind: RunKind,
        place: &str,
        branch: &str,
    ) -> Result<Box<dyn Holding + 'a>>;

    /// The process that answers now and its room; `None` where none
    /// answers, and nothing is started.
    fn room(&self, here: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>>;

    /// A subscription of `folders`, the host process started where none
    /// answers (design D6). `here` is the binding a test's host process
    /// reads in; the socket's takes none.
    fn subscribe<'a>(
        &'a self,
        here: Option<&'a Here<'a, 'a>>,
        folders: Vec<Added>,
    ) -> Result<Box<dyn Subscription + 'a>>;

    /// A wait's step of `ask`, from the folder `folder` (the change
    /// `sluice-host-answers-every-read`, design D8). `here` as for
    /// [`HostProcess::subscribe`].
    fn wait(
        &self,
        here: Option<&Here<'_, '_>>,
        folder: &Path,
        ask: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep>;

    /// A pass of `repository` now, at the asker's time on `clock`, once it ends:
    /// a follower's ask at its tick where passes are taken only when asked
    /// (D14). `here` as for [`HostProcess::subscribe`].
    fn pass_of(
        &self,
        here: Option<&Here<'_, '_>>,
        repository: &Path,
        clock: &dyn sluice_core::Clock,
    ) -> Result<Report>;
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
        self.client()?
            .pass(&here.repository()?, Some(here.clock.now()))
    }

    /// A command reads through `sluice host`'s reads, never a reading of
    /// its own (the change `sluice-host-answers-every-read`, design D1).
    fn read(&self, _: &Here<'_, '_>, _: bool) -> Result<Reading> {
        Err(anyhow!("a read is asked of sluice's host process"))
    }

    /// A session's or a preview's slot is held in `sluice host`, which
    /// runs its command (the change `sluice-host-runs-a-sessions-checks`,
    /// design D3): a command asks none.
    fn hold<'a>(
        &'a self,
        _: &Here<'a, '_>,
        _: RunKind,
        _: &str,
        _: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        Err(anyhow!(
            "a session's slot is held in sluice's host process, which runs its command"
        ))
    }

    /// `sluice host` renders the room with each read (design D9): a
    /// command asks it for none of its own.
    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(None)
    }

    fn subscribe<'a>(
        &'a self,
        _: Option<&'a Here<'a, 'a>>,
        folders: Vec<Added>,
    ) -> Result<Box<dyn Subscription + 'a>> {
        let c = self.client()?.subscribe(folders)?;
        self.mode.set(Some(c.named.asked_only));
        Ok(Box::new(Subscribed(c)))
    }

    fn pass_of(
        &self,
        _: Option<&Here<'_, '_>>,
        repository: &Path,
        clock: &dyn sluice_core::Clock,
    ) -> Result<Report> {
        self.client()?.pass(repository, Some(clock.now()))
    }

    fn wait(
        &self,
        _: Option<&Here<'_, '_>>,
        folder: &Path,
        ask: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        // The worktree as `sluice host` finds it, from its own folder.
        let ask = super::protocol::WaitAsk {
            worktree: ask
                .worktree
                .as_ref()
                .map(|w| folder.join(w).display().to_string()),
            ..ask.clone()
        };
        self.client()?.wait(folder, &ask)
    }
}

/// A subscription on the host process's socket (design D6).
struct Subscribed(Connection);

impl Subscription for Subscribed {
    fn asked_only(&self) -> bool {
        self.0.named.asked_only
    }

    fn say(&mut self, ask: &SubscriptionAsk) -> Result<()> {
        self.0.say(ask)
    }

    fn next(&mut self, wait: Duration) -> Result<Option<SubscriptionLine>> {
        match self.0.line(Some(wait))? {
            Some(SubscriptionLine::Error(words)) => Err(anyhow!("{words}")),
            line => Ok(line),
        }
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

/// 129 (design D16): the reading of the repository `here` binds that
/// `sluice host` answers a read with, current within its tick. One that did
/// not come within the round's retry bound and a tick is said on stderr,
/// with when `sluice host` last read the repository, and the read is
/// answered from the records as the host holds them all the same (the
/// change `sluice-host-answers-every-read`, design D4). A
/// current reading whose ask a remote refused says so in the remote's
/// words (133), the records read as last fetched. With `afresh`, the
/// reading is one begun after the ask.
pub fn read(here: &Here<'_, '_>, afresh: bool) -> Option<Reading> {
    match here.host.host_process().read(here, afresh) {
        Ok(r) if r.current => {
            if let Some(words) = &r.refused {
                crate::telemetry::warn(&format!(
                    "sluice: warning: sluice's host process could not read the repository's remote, so this reads it as last fetched: {words}"
                ));
            }
            Some(r)
        }
        Ok(r) => {
            let when = match &r.ended {
                Some(at) => format!("last read it at {at}"),
                None => "has not read it yet".to_string(),
            };
            crate::telemetry::warn(&format!(
                "sluice: warning: sluice's host process {when}; this reads the repository as the host holds it"
            ));
            Some(r)
        }
        Err(e) => {
            crate::telemetry::warn(&format!(
                "sluice: warning: sluice's host process could not be reached; this reads the repository as the host holds it: {e:#}"
            ));
            None
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

    fn read(&self, _: &Here<'_, '_>, _: bool) -> Result<Reading> {
        Err(anyhow!(
            "a reading is asked of the host process, not from within it"
        ))
    }

    fn hold<'a>(
        &'a self,
        _: &Here<'a, '_>,
        _: RunKind,
        _: &str,
        _: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        Err(anyhow!(
            "a session's room is asked of the host process, not from within it"
        ))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(Some((self.named.clone(), super::view_of(&self.room)?)))
    }

    fn subscribe<'a>(
        &'a self,
        _: Option<&'a Here<'a, 'a>>,
        _: Vec<Added>,
    ) -> Result<Box<dyn Subscription + 'a>> {
        Err(anyhow!("the host process subscribes to nothing of its own"))
    }

    fn pass_of(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &dyn sluice_core::Clock,
    ) -> Result<Report> {
        Err(anyhow!(
            "a pass is asked of the host process, not from within it"
        ))
    }

    fn wait(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        Err(anyhow!(
            "a wait is asked of the host process, not from within it"
        ))
    }
}

/// No host process, for a worktree's `sluice release` where none answers:
/// it frees what it reads of the records in its own process and asks
/// nothing, so it starts none (record 0120 as amended 2026-11-17; rule
/// core.10).
pub struct Unasked;

impl HostProcess for Unasked {
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
        Err(anyhow!("no host process is asked"))
    }

    fn pass(&self, _: &Here<'_, '_>) -> Result<Report> {
        Err(anyhow!("no host process is asked"))
    }

    fn read(&self, _: &Here<'_, '_>, _: bool) -> Result<Reading> {
        Err(anyhow!("no host process is asked"))
    }

    fn hold<'a>(
        &'a self,
        _: &Here<'a, '_>,
        _: RunKind,
        _: &str,
        _: &str,
    ) -> Result<Box<dyn Holding + 'a>> {
        Err(anyhow!("no host process is asked"))
    }

    fn room(&self, _: &Here<'_, '_>) -> Result<Option<(Named, RoomView)>> {
        Ok(None)
    }

    fn subscribe<'a>(
        &'a self,
        _: Option<&'a Here<'a, 'a>>,
        _: Vec<Added>,
    ) -> Result<Box<dyn Subscription + 'a>> {
        Err(anyhow!("no host process is asked"))
    }

    fn pass_of(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &dyn sluice_core::Clock,
    ) -> Result<Report> {
        Err(anyhow!("no host process is asked"))
    }

    fn wait(
        &self,
        _: Option<&Here<'_, '_>>,
        _: &Path,
        _: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        Err(anyhow!("no host process is asked"))
    }
}
