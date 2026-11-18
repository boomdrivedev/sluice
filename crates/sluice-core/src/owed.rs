//! 51: what a folder's readers are owed, read from the branches'
//! deliveries with weir's `owed`, and each hand-over of it recorded, one
//! write per branch under its lock (weir 103, 105, 109, 111; record 0102;
//! design D6).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use sluice_seams::{Decided, Guard, Submitter};

use crate::line::{submission_of, Line};
use crate::ops::Unread;
use crate::overview::Here;
use crate::records::{State, Submission};
use crate::Sluice;

/// 51: one outcome owed to a reader a folder reads as, as read under its
/// branch's lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwedOutcome {
    pub branch: String,
    /// Its submission's record as read, live or finished.
    pub submission: Submission,
    pub decided: Decided,
    /// Its key, `<id>:<outcome>` (design D2).
    pub key: String,
    /// The reader it is owed to, as its delivery names it: a worktree's
    /// branch, or a request's `pull/<n>`.
    pub reader: String,
    /// Its delivery, owed or expired, with its hand-overs to the reader,
    /// as weir's `owed` read it.
    pub owed: weir_delivery::OwedDelivery,
    /// Whether any branch's store knows the reader's name: whether a reader
    /// has ever confirmed under it (weir 106).
    pub known: bool,
}

/// Which of a folder's owed outcomes a read takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Collect {
    /// Every one owed or expired, on every branch: `sluice inbox`.
    All,
    /// Only those no way has handed to their reader yet: `sluice inbox
    /// --new` (design D8).
    New,
    /// Submission `id` of `branch` alone, where it is owed: a wait's own.
    One { branch: String, id: u64 },
}

/// 51, 135: a folder's owed outcomes, with the lock of each branch that
/// holds one, so no round pushes one of them while they are handed over.
/// [`Owed::hand`] records the hand-over; dropped without it, it records
/// nothing and lets the locks go, and each stays owed with no hand-over by
/// it, to be handed over again under the same key.
pub struct Owed<'s, 'a> {
    sluice: &'s Sluice<'a>,
    /// When they are handed over: what [`Owed::hand`] records.
    pub at: String,
    /// In branch, then submission, order.
    pub outcomes: Vec<OwedOutcome>,
    /// The branches it could not read; every other was read (rule core.11).
    pub unread: Vec<Unread>,
    _locks: Vec<Box<dyn Guard + 'a>>,
}

impl Owed<'_, '_> {
    /// 51: record each outcome handed to its reader `by` a way, reaching
    /// `to`, with weir's `hand`, and confirmed with it where `confirms`,
    /// one write per branch, each logged `handed`, and `confirmed` where it
    /// confirms. One its reader confirmed since it was read, by another
    /// host, is left (weir 103; design D5, D6).
    pub fn hand(self, by: &str, to: &str, confirms: bool) -> Result<()> {
        let mut by_branch: BTreeMap<&str, Vec<&OwedOutcome>> = BTreeMap::new();
        for o in &self.outcomes {
            by_branch.entry(&o.branch).or_default().push(o);
        }
        for (branch, outcomes) in by_branch {
            self.sluice.update(branch, |l| {
                for o in &outcomes {
                    l.hand(
                        o.submission.id,
                        &o.key,
                        &o.reader,
                        by,
                        to,
                        confirms,
                        &self.at,
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}

impl<'a> Sluice<'a> {
    /// 51, 135: the outcomes `me`'s readers are owed on any branch, as
    /// `collect` takes them: each delivery weir's `owed` reads as owed or
    /// expired at this time to a reader the folder reads as, its branch or
    /// a request's opened from it, by [`Here::made`]'s reading of the
    /// submission's work; never one by name (124), withdrawn or not
    /// decided. Read first with no lock, then again under the lock of each
    /// branch holding one, taken in the branches' order so two collectors
    /// never wait on each other. A branch it cannot read is among the
    /// unread and the others are read still (rule core.11); for one
    /// submission, its branch unread fails the read. Nothing is written
    /// until [`Owed::hand`] (record 0102; design D6).
    pub fn owed<'s>(&'s self, me: &Submitter, collect: Collect) -> Result<Owed<'s, 'a>> {
        let at = self.now();
        let here = Here::on(&me.worktree);
        let branches = match &collect {
            Collect::One { branch, .. } => vec![branch.clone()],
            _ => {
                let mut all = self.seams.state.branches()?;
                all.sort();
                all.dedup();
                all
            }
        };
        let mut unread = Vec::new();
        let mut holding = Vec::new();
        let mut known = self.known_readers();
        for b in branches {
            let found = self
                .read_line(&b)
                .and_then(|line| self.owed_on(&line, &here, &collect, &at, &known));
            match found {
                Ok(found) if found.is_empty() => {}
                Ok(_) => holding.push(b),
                Err(e) if matches!(collect, Collect::One { .. }) => return Err(e),
                Err(e) => unread.push(Unread::of(&b, &e)),
            }
        }
        let state = self.seams.state;
        let mut locks = Vec::new();
        let mut outcomes = Vec::new();
        for b in holding {
            locks.push(state.wait_lock(&b)?);
            let found = self.read_line(&b).and_then(|line| {
                known.extend(known_in(&line));
                self.owed_on(&line, &here, &collect, &at, &known)
            });
            match found {
                Ok(found) => outcomes.extend(found),
                Err(e) if matches!(collect, Collect::One { .. }) => return Err(e),
                Err(e) => unread.push(Unread::of(&b, &e)),
            }
        }
        unread.sort_by(|x, y| x.branch.cmp(&y.branch));
        Ok(Owed {
            sluice: self,
            at,
            outcomes,
            unread,
            _locks: locks,
        })
    }

    /// 51: every reader's name some branch's store knows, from the
    /// branches' records as the host holds them, a branch it cannot read
    /// passed over (weir 106; design D6).
    pub fn known_readers(&self) -> BTreeSet<String> {
        let Ok(branches) = self.seams.state.branches() else {
            return BTreeSet::new();
        };
        branches
            .iter()
            .filter_map(|b| {
                let snapshot = self.seams.state.read_held(b).ok()?;
                Line::load(b, snapshot).ok()
            })
            .flat_map(|line| known_in(&line))
            .collect()
    }

    /// The outcomes on `line`'s branch owed to the folder `here`'s readers
    /// at `at`, as `collect` takes them, in submission order.
    fn owed_on(
        &self,
        line: &Line,
        here: &Here<'_>,
        collect: &Collect,
        at: &str,
        known: &BTreeSet<String>,
    ) -> Result<Vec<OwedOutcome>> {
        let branch = line.branch.as_str();
        let owed = weir_delivery::owed(line.deliveries(), at)
            .map_err(|refused| anyhow!("reading {branch}'s deliveries: {refused}"))?;
        let mut found = Vec::new();
        for r in owed.readers {
            let reader = r.reader.as_str();
            // Only a name a folder can read as: its branch, or a request's.
            if reader != here.branch() && !reader.starts_with("pull/") {
                continue;
            }
            for d in r.deliveries {
                let key = d.delivery.key().as_str();
                let Some(id) = submission_of(key) else {
                    continue;
                };
                if let Collect::One { id: only, .. } = collect {
                    if id != *only {
                        continue;
                    }
                }
                if *collect == Collect::New && !d.handovers.is_empty() {
                    continue;
                }
                let s = match line.subs.get(&id) {
                    Some(s) => s.clone(),
                    None => match self.finished_sub(branch, id)? {
                        Some(s) => s,
                        None => continue,
                    },
                };
                // 152, record 0124: a held landing's refusal is owed as an
                // outcome is, and handed over as one, deciding none.
                if !matches!(
                    s.state,
                    State::Landed | State::Failed | State::Conflict | State::Refused
                ) || s.from.worktree != reader
                    || !self.made(here, &s.from)
                {
                    continue;
                }
                let item = line.doc.outbox.iter().find(|o| o.id == key);
                // A refusal the round has pushed is read from the landing
                // that holds it, as its outbox item named it.
                let refused = || {
                    line.refusal_of(id).map(|r| Decided::Refused {
                        words: r.words.clone(),
                    })
                };
                let Some(decided) = item
                    .map(|o| o.outcome.to_decided())
                    .or_else(|| s.decided())
                    .or_else(refused)
                else {
                    continue;
                };
                found.push(OwedOutcome {
                    branch: branch.into(),
                    submission: s,
                    decided,
                    key: key.into(),
                    reader: reader.into(),
                    known: r.known || known.contains(reader),
                    owed: d,
                });
            }
        }
        found.sort_by_key(|o| o.submission.id);
        Ok(found)
    }
}

/// The readers' names `line`'s deliveries know.
fn known_in(line: &Line) -> Vec<String> {
    line.deliveries()
        .iter()
        .filter_map(|r| match r {
            weir_delivery::Record::Known(k) => Some(k.reader.to_string()),
            _ => None,
        })
        .collect()
}

/// 51: who confirms: the readers a folder reads as, or the operator, for
/// each of a delivery's own readers (S77; design D7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmAs {
    /// The folder of the submitter a submit there would record.
    Folder(Submitter),
    /// The operator, for the delivery's own readers, wherever it runs.
    Operator,
}

/// 51: what [`Sluice::confirm`] answers of one key (weir 102; design D7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// Confirmed by this call for `reader`, at `at`: submission `id`'s
    /// outcome on `branch`.
    Confirmed {
        key: String,
        branch: String,
        id: u64,
        reader: String,
        at: String,
    },
    /// The reader had confirmed it, first at `at`; nothing written.
    Before {
        key: String,
        branch: String,
        id: u64,
        reader: String,
        at: String,
    },
    /// No branch read holds a delivery under it.
    NotFound { key: String },
    /// Owed to readers the folder does not read as, which it names.
    Another {
        key: String,
        branch: String,
        readers: Vec<String>,
    },
}

/// 51: [`Sluice::confirm`]'s answers, one per key in the order given, and
/// the branches it could not read, where a key not found may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmations {
    pub answers: Vec<Confirmation>,
    pub unread: Vec<Unread>,
}

impl Sluice<'_> {
    /// 51: confirm each of `keys` on whichever branch holds a delivery
    /// under it, with weir's `confirm`: for the reader the folder reads
    /// as, by [`Here::made`]'s reading of the submission's work, or, as
    /// the operator, for each of the delivery's readers that has not
    /// confirmed it; each confirmation written under the branch's lock,
    /// one write per branch, and logged `confirmed`, `by` `confirm` or
    /// `operator`. A key the reader confirmed already says when and writes
    /// nothing; one owed to another reader is refused, naming it; one no
    /// branch read holds is not found, and a branch it could not read is
    /// named (weir 102, 104, 106, 110; S77; rule core.11; design D7).
    pub fn confirm(&self, keys: &[String], who: &ConfirmAs) -> Result<Confirmations> {
        let mut branches = self.seams.state.branches()?;
        branches.sort();
        branches.dedup();
        let mut unread = Vec::new();
        let mut at_branch: BTreeMap<&str, String> = BTreeMap::new();
        for b in &branches {
            match self.read_line(b) {
                Ok(line) => {
                    for key in keys {
                        if !at_branch.contains_key(key.as_str()) && delivery_in(&line, key) {
                            at_branch.insert(key, b.clone());
                        }
                    }
                }
                Err(e) => unread.push(Unread::of(b, &e)),
            }
        }
        let mut answered: BTreeMap<&str, Confirmation> = BTreeMap::new();
        let mut by_branch: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (key, branch) in &at_branch {
            by_branch.entry(branch).or_default().push(key);
        }
        let now = self.now();
        for (branch, keyed) in by_branch {
            let _lock = self.seams.state.wait_lock(branch)?;
            let mut got = Vec::new();
            self.update(branch, |l| {
                got = keyed
                    .iter()
                    .map(|key| self.confirm_on(l, key, who, &now))
                    .collect::<Result<Vec<_>>>()?;
                Ok(())
            })?;
            for (key, answer) in keyed.into_iter().zip(got) {
                answered.insert(key, answer);
            }
        }
        let answers = keys
            .iter()
            .map(|key| {
                answered
                    .remove(key.as_str())
                    .unwrap_or_else(|| Confirmation::NotFound { key: key.clone() })
            })
            .collect();
        unread.sort_by(|x, y| x.branch.cmp(&y.branch));
        Ok(Confirmations { answers, unread })
    }

    /// The confirmation of `key` on `l` by `who` at `now`, added to `l`
    /// with its `confirmed` lines.
    fn confirm_on(
        &self,
        l: &mut Line,
        key: &str,
        who: &ConfirmAs,
        now: &str,
    ) -> Result<Confirmation> {
        let branch = l.branch.clone();
        let id = submission_of(key).unwrap_or_default();
        let Some(readers) = readers_of(l, key) else {
            return Ok(Confirmation::NotFound { key: key.into() });
        };
        let (mine, by): (Vec<String>, &str) = match who {
            ConfirmAs::Operator => (readers.clone(), "operator"),
            ConfirmAs::Folder(me) => {
                let here = Here::on(&me.worktree);
                let from = match l.subs.get(&id) {
                    Some(s) => Some(s.from.clone()),
                    None => self.finished_sub(&branch, id)?.map(|s| s.from),
                };
                let mine = readers
                    .iter()
                    .filter(|r| match &from {
                        Some(from) => from.worktree == **r && self.made(&here, from),
                        None => **r == me.worktree,
                    })
                    .cloned()
                    .collect();
                (mine, "confirm")
            }
        };
        if mine.is_empty() {
            return Ok(Confirmation::Another {
                key: key.into(),
                branch,
                readers,
            });
        }
        let mut answer = None;
        for reader in &mine {
            let confirmed = weir_delivery::confirm(l.deliveries(), key, reader, now)
                .map_err(|refused| anyhow!("confirming {key} on {branch}: {refused}"))?;
            match confirmed {
                weir_delivery::Confirmed::Add(add) => {
                    l.add_deliveries(&add);
                    l.log(
                        now,
                        crate::records::Event::Confirmed {
                            id,
                            key: key.into(),
                            reader: reader.clone(),
                            by: by.into(),
                        },
                    );
                    answer = Some(Confirmation::Confirmed {
                        key: key.into(),
                        branch: branch.clone(),
                        id,
                        reader: reader.clone(),
                        at: now.into(),
                    });
                }
                weir_delivery::Confirmed::Already(first) => {
                    answer.get_or_insert(Confirmation::Before {
                        key: key.into(),
                        branch: branch.clone(),
                        id,
                        reader: reader.clone(),
                        at: first.to_string(),
                    });
                }
            }
        }
        Ok(answer.expect("a reader confirmed or had"))
    }
}

/// Whether `line`'s deliveries hold one under `key`.
fn delivery_in(line: &Line, key: &str) -> bool {
    readers_of(line, key).is_some()
}

/// The readers of the delivery `line` holds under `key`, the first under
/// it, which stands (weir 101).
fn readers_of(line: &Line, key: &str) -> Option<Vec<String>> {
    line.deliveries().iter().find_map(|r| match r {
        weir_delivery::Record::Delivery(d) if d.key().as_str() == key => {
            Some(d.readers().iter().map(|r| r.to_string()).collect())
        }
        _ => None,
    })
}

/// 51: where a decided submission's delivery stands, as a document gives
/// it (weir 104 to 107; design D13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryView {
    /// As the branch's deliveries hold it.
    Kept {
        key: String,
        reader: String,
        /// `owed`, `expired` or `done`.
        state: &'static str,
        recorded_at: String,
        expires_at: String,
        /// Each hand-over to the reader, its way and when, in order.
        handed: Vec<(String, String)>,
        /// When the reader confirmed it, once it has.
        confirmed: Option<String>,
        /// Whether any reader has confirmed under the reader's name.
        known: bool,
    },
    /// Done and let go of: nothing else of it is kept (weir 107).
    LetGo { key: String },
}

impl DeliveryView {
    /// The document's `delivery`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            DeliveryView::LetGo { key } => serde_json::json!({ "key": key, "state": "done" }),
            DeliveryView::Kept {
                key,
                reader,
                state,
                recorded_at,
                expires_at,
                handed,
                confirmed,
                known,
            } => serde_json::json!({
                "key": key,
                "reader": reader,
                "state": state,
                "recorded_at": recorded_at,
                "expires_at": expires_at,
                "handed": handed
                    .iter()
                    .map(|(by, at)| serde_json::json!({ "by": by, "at": at }))
                    .collect::<Vec<_>>(),
                "confirmed": confirmed,
                "known": known,
            }),
        }
    }
}

/// 51: one reader with what it is owed on a branch, as the branch's status
/// shows it (weir 104 to 106, 111; S77; design D13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderView {
    pub reader: String,
    /// Whether any reader has confirmed under its name, on any branch.
    pub known: bool,
    pub deliveries: Vec<OwedView>,
}

/// 51: one delivery a reader has not confirmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwedView {
    pub key: String,
    /// The submission its key names.
    pub id: Option<u64>,
    pub expired: bool,
    pub expires_at: String,
    /// Each hand-over to the reader, its way and when, in order.
    pub handed: Vec<(String, String)>,
}

impl OwedOutcome {
    /// 51: its delivery as a hand-over `by` a way at `at` leaves it,
    /// confirmed with it where `confirms` (design D8, D13).
    pub fn left(&self, by: &str, at: &str, confirms: bool) -> DeliveryView {
        let d = &self.owed.delivery;
        let mut handed = handovers(&self.owed);
        handed.push((by.into(), at.into()));
        DeliveryView::Kept {
            key: self.key.clone(),
            reader: self.reader.clone(),
            state: match (confirms, self.owed.expired) {
                (true, _) => "done",
                (false, true) => "expired",
                (false, false) => "owed",
            },
            recorded_at: d.recorded_at().to_string(),
            expires_at: d.expires_at().to_string(),
            handed,
            confirmed: confirms.then(|| at.to_string()),
            known: self.known || confirms,
        }
    }
}

fn handovers(owed: &weir_delivery::OwedDelivery) -> Vec<(String, String)> {
    owed.handovers
        .iter()
        .map(|h| (h.by.to_string(), h.at.to_string()))
        .collect()
}

impl Sluice<'_> {
    /// 51: where `sub`'s delivery stands, read from its branch's records
    /// at this time: none for a submission by name, a withdrawn one or one
    /// not decided, nor for one decided before deliveries were recorded
    /// that is live still; one finished whose delivery the records no
    /// longer hold was done and let go of (124; weir 107; design D13). One
    /// whose landing the forge refused gives the refusal's, which is owed
    /// though it decides nothing (152, record 0124).
    pub fn delivery_of(&self, sub: &Submission) -> Result<Option<DeliveryView>> {
        if sub.from.by_name()
            || !matches!(
                sub.state,
                State::Landed | State::Failed | State::Conflict | State::Refused
            )
        {
            return Ok(None);
        }
        let Some(key) = sub.notification() else {
            return Ok(None);
        };
        let line = self.read_line(&sub.branch)?;
        let records = line.deliveries();
        let Some(delivery) = records.iter().find_map(|r| match r {
            weir_delivery::Record::Delivery(d) if d.key().as_str() == key => Some(d),
            _ => None,
        }) else {
            return Ok((!line.subs.contains_key(&sub.id)).then_some(DeliveryView::LetGo { key }));
        };
        let reader = delivery
            .readers()
            .first()
            .map(|r| r.to_string())
            .unwrap_or_default();
        let at = self.now();
        let owed = weir_delivery::owed(records, &at)
            .map_err(|refused| anyhow!("reading {}'s deliveries: {refused}", sub.branch))?;
        let mine = owed.readers.iter().find(|o| o.reader.as_str() == reader);
        let standing = mine.and_then(|o| {
            o.deliveries
                .iter()
                .find(|d| d.delivery.key().as_str() == key)
        });
        let confirmed = records.iter().find_map(|r| match r {
            weir_delivery::Record::Confirmation(c)
                if c.key.as_str() == key && c.reader.as_str() == reader =>
            {
                Some(c.at.to_string())
            }
            _ => None,
        });
        let handed = records
            .iter()
            .filter_map(|r| match r {
                weir_delivery::Record::Handover(h)
                    if h.key.as_str() == key && h.reader.as_str() == reader =>
                {
                    Some((h.by.to_string(), h.at.to_string()))
                }
                _ => None,
            })
            .collect();
        let known = confirmed.is_some()
            || mine.is_some_and(|o| o.known)
            || self.known_readers().contains(&reader);
        Ok(Some(DeliveryView::Kept {
            state: match standing {
                None => "done",
                Some(d) if d.expired => "expired",
                Some(_) => "owed",
            },
            recorded_at: delivery.recorded_at().to_string(),
            expires_at: delivery.expires_at().to_string(),
            key,
            reader,
            handed,
            confirmed,
            known,
        }))
    }

    /// 51: each reader owed something on `branch` at this time, owed or
    /// expired, with its deliveries and their hand-overs, in the order
    /// weir's `owed` gives them; a reader known where any branch's store
    /// knows its name (weir 104 to 106, 111; S77; design D13).
    pub fn owed_on_branch(&self, branch: &str) -> Result<Vec<ReaderView>> {
        let line = self.read_line(branch)?;
        let owed = weir_delivery::owed(line.deliveries(), &self.now())
            .map_err(|refused| anyhow!("reading {branch}'s deliveries: {refused}"))?;
        if owed.readers.is_empty() {
            return Ok(Vec::new());
        }
        let known = self.known_readers();
        Ok(owed
            .readers
            .into_iter()
            .map(|r| ReaderView {
                known: r.known || known.contains(r.reader.as_str()),
                reader: r.reader.to_string(),
                deliveries: r
                    .deliveries
                    .iter()
                    .map(|d| OwedView {
                        key: d.delivery.key().to_string(),
                        id: submission_of(d.delivery.key().as_str()),
                        expired: d.expired,
                        expires_at: d.delivery.expires_at().to_string(),
                        handed: handovers(d),
                    })
                    .collect(),
            })
            .collect())
    }
}
