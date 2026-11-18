//! The release of a combination's checkout (123, 25, 34; ADR 0055): one
//! path for every place the core made for a candidate, whether a round
//! releases it once its verdict is read, `recover` releases a place left
//! from a run that never started, or a returning host releases what it
//! left before it claims (81).
//!
//! A release that fails is an outcome, never an error: the place stays,
//! named in the branch's records with what failed and how many times it
//! was tried, and the round goes on with the branch's work. It is tried
//! once a round until the branch's `release_retries` are spent, then held
//! until the operator releases it by name or accepts its failure. A place
//! whose working copy is gone has the release its last run stored run
//! once, from its scratch directory, and is freed whatever that did; it is
//! never tried again.

use anyhow::Result;

use crate::line::Line;
use crate::records::{Event, FailedRelease};
use crate::step::RELEASE_RETRIES;
use crate::Sluice;

/// What became of a combination's checkout that was released, as the
/// round or `sluice release` that released it reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseOutcome {
    pub branch: String,
    pub place: String,
    pub how: Releasing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Releasing {
    /// Its release ran, and it is gone.
    Released,
    /// Its working copy was gone: the release its runs stored was run
    /// once, when there was one, and it was freed whatever that did.
    Gone { ran: bool, failed: Option<String> },
    /// Freed with nothing run, at the operator's accepting that its
    /// release failed: what failed last, if its release ever did.
    Accepted(Option<String>),
    /// Its release failed on try `tries` of the `of` it is given.
    Failed { failed: String, tries: u32, of: u32 },
    /// Its release failed and is tried no more: held.
    Held { failed: String, tries: u32 },
}

impl Releasing {
    /// The place is no longer there to release.
    pub fn freed(&self) -> bool {
        matches!(
            self,
            Releasing::Released | Releasing::Gone { .. } | Releasing::Accepted(_)
        )
    }
}

/// What `released` says was accepted of a place whose release never
/// failed: the operator's word is the ruling (123).
const NEVER_FAILED: &str = "no release had failed";

/// A place leaves the branch's lists of places to release.
fn forget(l: &mut Line, place: &str) {
    l.doc.release.retain(|p| p != place);
    l.doc.failed_releases.retain(|f| f.place != place);
}

impl Sluice<'_> {
    /// 123, 56: how many more times a failed release of a place on
    /// `branch` is tried, read from the branch's current revision, and the
    /// host to name as the place's maker on a branch whose records are on
    /// its remote. The default bound when the branch names none or cannot
    /// be read, so a branch that cannot be read never stops a release.
    fn release_rules(&self, branch: &str) -> (u32, Option<String>) {
        let rules = match self.seams.branch.read(branch) {
            Ok(Some(tip)) => self.rules_at(branch, &tip).ok().flatten(),
            _ => None,
        };
        match rules {
            Some(r) => (r.release_retries, r.signed.then(|| self.peer.name.clone())),
            None => (RELEASE_RETRIES, None),
        }
    }

    /// How many more times a failed release of a place on `branch` is
    /// tried (123).
    pub(crate) fn release_retries(&self, branch: &str) -> u32 {
        self.release_rules(branch).0
    }

    /// 123, 25, 34: release the combination's checkout `place` on
    /// `branch`, under the branch's lock. A checkout whose working copy is
    /// gone has its stored release run once and is freed whatever it did,
    /// logged `gone` with how its release went; one released is logged
    /// `released`; either leaves the branch's lists, and is never tried
    /// again. A release that
    /// fails counts a try and is logged `release_failed`; once its tries
    /// pass the branch's `release_retries` it leaves `release` and is
    /// held. Only a failure to write the records is an error.
    pub fn release_made(&self, branch: &str, place: &str) -> Result<ReleaseOutcome> {
        let checkout = self.seams.checkout;
        let tried = checkout.gone(place).and_then(|gone| match gone {
            true => checkout.release_gone(place).map(|g| Releasing::Gone {
                ran: g.ran,
                failed: g.failed,
            }),
            false => checkout.release(place).map(|()| Releasing::Released),
        });
        let how = match tried {
            Ok(how) => {
                self.forget_released(branch, place, &how)?;
                how
            }
            // Record 0122 (design D3): a process that stopped part way
            // through the release reads nothing of it.
            Err(e) if sluice_seams::stopped(&e) => return Err(e),
            Err(e) => self.failed_release(branch, place, &format!("{e:#}"))?,
        };
        Ok(ReleaseOutcome {
            branch: branch.into(),
            place: place.into(),
            how,
        })
    }

    /// 123: the operator's release of the held checkout `place` on
    /// `branch`, under the branch's lock: its release run once more,
    /// whatever its tries, or with `accept` its failure accepted and the
    /// place freed with nothing run. `None` when the branch's records name
    /// no such place and this host holds none.
    pub fn release_held(
        &self,
        branch: &str,
        place: &str,
        accept: bool,
    ) -> Result<Option<ReleaseOutcome>> {
        let line = self.read_line(branch)?;
        let entry = line.doc.failed_releases.iter().find(|f| f.place == place);
        let here = self.seams.checkout.made()?.iter().any(|p| p == place);
        // A checkout is released on the host that made it.
        let elsewhere = entry
            .and_then(|f| f.host.as_deref())
            .is_some_and(|h| h != self.peer.name);
        if !here && (entry.is_none() || elsewhere) {
            return Ok(None);
        }
        let failure = entry.map(|f| f.failed.clone());
        if !accept {
            return self.release_made(branch, place).map(Some);
        }
        let how = match self.seams.checkout.free(place) {
            Ok(()) => Releasing::Accepted(failure),
            Err(e) => self.failed_release(branch, place, &format!("{e:#}"))?,
        };
        if how.freed() {
            self.forget_released(branch, place, &how)?;
        }
        Ok(Some(ReleaseOutcome {
            branch: branch.into(),
            place: place.into(),
            how,
        }))
    }

    /// 123: log in `branch`'s records that `place` was freed at the
    /// operator's accepting what failed of its release.
    pub(crate) fn log_accepted(
        &self,
        branch: &str,
        place: &str,
        worktree: Option<&str>,
        failed: Option<String>,
    ) -> Result<()> {
        if branch.is_empty() {
            return Ok(());
        }
        let now = self.now();
        let event = Event::Released {
            place: place.into(),
            worktree: worktree.map(str::to_string),
            accepted: Some(failed.unwrap_or_else(|| NEVER_FAILED.into())),
        };
        self.update(branch, |l| {
            l.log(&now, event.clone());
            Ok(())
        })
    }

    /// A place released, freed or accepted leaves the branch's lists, and
    /// the log says how.
    fn forget_released(&self, branch: &str, place: &str, how: &Releasing) -> Result<()> {
        let now = self.now();
        let event = match how {
            Releasing::Gone { ran, failed } => Event::Gone {
                place: place.into(),
                worktree: None,
                ran: *ran,
                failed: failed.clone(),
            },
            Releasing::Accepted(failed) => Event::Released {
                place: place.into(),
                worktree: None,
                accepted: Some(failed.clone().unwrap_or_else(|| NEVER_FAILED.into())),
            },
            _ => Event::Released {
                place: place.into(),
                worktree: None,
                accepted: None,
            },
        };
        self.update(branch, |l| {
            forget(l, place);
            l.log(&now, event.clone());
            Ok(())
        })
    }

    /// 123: one more failed try of `place`'s release, with what failed;
    /// held once its tries pass the branch's `release_retries`.
    fn failed_release(&self, branch: &str, place: &str, failed: &str) -> Result<Releasing> {
        let (retries, host) = self.release_rules(branch);
        let now = self.now();
        self.update(branch, |l| {
            let i = match l.doc.failed_releases.iter().position(|f| f.place == place) {
                Some(i) => i,
                None => {
                    l.doc.failed_releases.push(FailedRelease {
                        place: place.into(),
                        failed: String::new(),
                        tries: 0,
                        at: now.clone(),
                        stopped: false,
                        host: host.clone(),
                    });
                    l.doc.failed_releases.len() - 1
                }
            };
            let f = &mut l.doc.failed_releases[i];
            f.tries += 1;
            f.failed = failed.into();
            f.at = now.clone();
            f.stopped |= f.tries > retries;
            let (tries, held) = (f.tries, f.stopped);
            if held {
                l.doc.release.retain(|p| p != place);
            }
            l.log(
                &now,
                Event::ReleaseFailed {
                    place: place.into(),
                    failed: failed.into(),
                    tries,
                    held,
                },
            );
            Ok(match held {
                true => Releasing::Held {
                    failed: failed.into(),
                    tries,
                },
                false => Releasing::Failed {
                    failed: failed.into(),
                    tries,
                    of: retries.saturating_add(1),
                },
            })
        })
    }
}
