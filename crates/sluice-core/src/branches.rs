//! branches-1 104 to 106: sluice makes and removes the branches it lands
//! on, at the operator's or a planner's asking. Neither is a landing: no
//! commit is written, no verdict recorded and no gate's checks read (106).
//! Each takes the branch's lock, so no step acts on it meanwhile (rule
//! core.14), and each is one move under 45, refused when the branch was
//! elsewhere and never retried blind (rule core.5).
//!
//! 121: the operator accepts the revision a held branch is read at, under
//! the same lock; it moves nothing.

use anyhow::{bail, Result};
use sluice_seams::{Moved, Rev};

use crate::records::Event;
use crate::Sluice;

/// Why a branch was not made (104).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MakeRefusal {
    /// Another step holds the branch's lock.
    Busy,
    /// The branch to make it from does not exist.
    NoFrom,
    /// The branch to make it from is no branch sluice lands on.
    FromNotManaged,
    /// The policy at the revision it would start at gives it no gate (56).
    NotManaged,
    /// Its records hold entries still in a chain.
    Queued(Vec<u64>),
    /// It is present, at this revision (45).
    Present(Option<Rev>),
}

/// Why a branch was not removed (105).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveRefusal {
    /// Another step holds the branch's lock.
    Busy,
    /// There is no such branch.
    Absent,
    /// The branch its work landed into does not exist.
    NoInto,
    /// Entries on it are still in a chain.
    Queued(Vec<u64>),
    /// A landing on it was left unrecorded (S9): the next round settles it.
    LandingUnfinished,
    /// Its revision is not contained in the branch named.
    NotLanded(Rev),
    /// It moved after it was read (45).
    Elsewhere(Option<Rev>),
}

/// 121: a revision the operator accepted as a held branch's, and the
/// landing it lacked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acceptance {
    pub revision: Rev,
    pub lacked: Rev,
}

/// Why a revision was not accepted (121).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptRefusal {
    /// Another step holds the branch's lock.
    Busy,
    /// There is no such branch.
    Absent,
    /// Its reading and its records agree: it is not held. It reads this.
    NotHeld(Rev),
    /// It is held at `held`, and reads `read`: the revision given is not
    /// both.
    NotHeldAt { held: Rev, read: Rev },
}

/// Whether `given` names `rev`: all of it, or a prefix of seven hex
/// digits or more, as git takes one.
fn names(given: &str, rev: &Rev) -> bool {
    let given = given.to_ascii_lowercase();
    given.len() >= 7
        && given.chars().all(|c| c.is_ascii_hexdigit())
        && rev.as_str().starts_with(&given)
}

impl Sluice<'_> {
    /// 121, C.5 branch: record the operator's acceptance of `revision`,
    /// the one `branch` is held at and still read at, as the landing it
    /// stands on: every later read must contain it, and the next round
    /// forms on it. It moves no branch and touches no candidate.
    pub fn accept_branch(
        &self,
        branch: &str,
        revision: &str,
    ) -> Result<Result<Acceptance, AcceptRefusal>> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            return Ok(Err(AcceptRefusal::Busy));
        };
        let Some(read) = self.seams.branch.read(branch)? else {
            return Ok(Err(AcceptRefusal::Absent));
        };
        // A refusal writes nothing, not even records for a branch that has
        // none.
        let Some(hold) = self.read_line(branch)?.doc.held else {
            return Ok(Err(AcceptRefusal::NotHeld(read)));
        };
        if hold.read != read || !names(revision, &read) {
            return Ok(Err(AcceptRefusal::NotHeldAt {
                held: hold.read,
                read,
            }));
        }
        let now = self.now();
        self.update(branch, |l| {
            if l.doc.held.as_ref() != Some(&hold) {
                bail!("{branch}'s hold changed while its revision was accepted");
            }
            l.doc.landed = Some(read.clone());
            l.doc.held = None;
            l.log(
                &now,
                Event::Accepted {
                    revision: read.clone(),
                    lacked: hold.lacks.clone(),
                },
            );
            Ok(())
        })?;
        Ok(Ok(Acceptance {
            revision: read,
            lacked: hold.lacks,
        }))
    }

    /// 104: make `branch` at `from`'s current revision, when the policy
    /// there gives it a gate and its records hold nothing still queued;
    /// that revision is the one its reads must contain (121).
    pub fn make_branch(&self, branch: &str, from: &str) -> Result<Result<Rev, MakeRefusal>> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            return Ok(Err(MakeRefusal::Busy));
        };
        // Git and gix both take making a branch at the revision it already
        // holds as done, so a branch present is refused before the move;
        // the move's own expectation refuses one made since (46).
        if let Some(found) = self.seams.branch.read(branch)? {
            return Ok(Err(MakeRefusal::Present(Some(found))));
        }
        let Some(at) = self.seams.branch.read(from)? else {
            return Ok(Err(MakeRefusal::NoFrom));
        };
        if self.rules_at(from, &at)?.is_none() {
            return Ok(Err(MakeRefusal::FromNotManaged));
        }
        if self.rules_at(branch, &at)?.is_none() {
            return Ok(Err(MakeRefusal::NotManaged));
        }
        let queued = self.in_chain(branch)?;
        if !queued.is_empty() {
            return Ok(Err(MakeRefusal::Queued(queued)));
        }
        Ok(match self.seams.branch.make(branch, &at)? {
            Moved::Moved => {
                // 121: it stands on the revision it was made at until its
                // first landing, whatever a branch of its name removed
                // before had landed.
                self.update(branch, |l| {
                    l.doc.landed = Some(at.clone());
                    l.doc.held = None;
                    Ok(())
                })?;
                Ok(at)
            }
            Moved::Elsewhere(found) => Err(MakeRefusal::Present(found)),
            // 152: the forge refused the branch's making.
            Moved::Refused(words) => bail!("the forge refused making {branch}: {words}"),
        })
    }

    /// 105: remove `branch` from the revision read, when nothing on it is
    /// still in a chain, no landing on it is unrecorded, and that revision
    /// is contained in `into`'s current one. Its records stay.
    pub fn remove_branch(&self, branch: &str, into: &str) -> Result<Result<Rev, RemoveRefusal>> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            return Ok(Err(RemoveRefusal::Busy));
        };
        let Some(tip) = self.seams.branch.read(branch)? else {
            return Ok(Err(RemoveRefusal::Absent));
        };
        if self.read_line(branch)?.doc.landing.is_some() {
            return Ok(Err(RemoveRefusal::LandingUnfinished));
        }
        let queued = self.in_chain(branch)?;
        if !queued.is_empty() {
            return Ok(Err(RemoveRefusal::Queued(queued)));
        }
        let Some(target) = self.seams.branch.read(into)? else {
            return Ok(Err(RemoveRefusal::NoInto));
        };
        if !self.seams.branch.contains(&target, &tip)? {
            return Ok(Err(RemoveRefusal::NotLanded(tip)));
        }
        Ok(match self.seams.branch.remove(branch, &tip)? {
            Moved::Moved => {
                // 121: no reader sees a hold on a branch that is gone.
                if self.read_line(branch)?.doc.held.is_some() {
                    self.update(branch, |l| {
                        l.doc.held = None;
                        Ok(())
                    })?;
                }
                Ok(tip)
            }
            Moved::Elsewhere(found) => Err(RemoveRefusal::Elsewhere(found)),
            // 152: the forge refused the branch's removal.
            Moved::Refused(words) => bail!("the forge refused removing {branch}: {words}"),
        })
    }

    /// The entries on `branch` still in a chain: neither landed, withdrawn,
    /// blamed nor conflicting.
    fn in_chain(&self, branch: &str) -> Result<Vec<u64>> {
        Ok(self
            .read_line(branch)?
            .subs
            .values()
            .filter(|s| !s.state.is_out())
            .map(|s| s.id)
            .collect())
    }
}
