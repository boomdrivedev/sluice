//! A branch's queue's home, and a move between homes (record 0122; 58,
//! 59, 62, 63; C.6; design D1 to D5 of
//! `a-queues-home-is-written-in-its-records`).
//!
//! The records seam's bindings route a branch by its `home.json` and carry
//! its live records from one home to another when the core asks
//! ([`sluice_seams::State::carry`]). What a carry writes is decided here,
//! once, so every binding carries alike: each binding reaches a home as a
//! [`HomeStore`] and calls [`carry`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use sluice_seams::{Carried, Rev, Snapshot, Written};

use crate::line::Line;
use crate::policy::Rules;
use crate::records::{
    finished_name, CameFrom, CarriedTo, Closed, Event, FinishedLine, HomeDoc, HomeState,
    OutboxItem, Submission, HERE,
};

/// Design D3: the home the rules at a branch's head name for its queue: a
/// branch whose records the policy keeps on its remote, `signed` and with a
/// `remote`, that remote; any other the host's own files.
pub fn named(rules: &Rules) -> String {
    match (&rules.remote, rules.signed) {
        (Some(remote), true) => remote.clone(),
        _ => HERE.into(),
    }
}

/// Design D3: whether `a` and `b` are one submission as two homes hold it,
/// told by its work, its head and when it was submitted, whatever number
/// each gives it.
pub fn same_work(a: &Submission, b: &Submission) -> bool {
    a.head == b.head
        && a.submitted == b.submitted
        && a.from.worktree == b.from.worktree
        && a.from.host == b.from.host
}

/// Record 0122 (design D2): two homes of `branch`, both open or moving,
/// refused by every read, write and lock of it, naming both (26).
pub fn two_homes(branch: &str, one: &str, other: &str) -> anyhow::Error {
    anyhow!(
        "{branch}'s queue is open in two homes, {} and {}: each holds its \
         own lock and numbers, so nothing reads or writes {branch} until \
         one is closed",
        describe(one),
        describe(other)
    )
}

/// A home as a person reads it.
pub fn describe(home: &str) -> String {
    match home {
        HERE => "this host's own files".into(),
        remote => remote.to_string(),
    }
}

/// One home of a branch, as a binding reaches it for a carry (design D3).
pub trait HomeStore {
    /// Its name: [`HERE`] or a remote's.
    fn name(&self) -> String;
    /// The branch's live records there.
    fn read(&self) -> Result<Snapshot>;
    /// One file of its finished records there, by its name under
    /// [`sluice_seams::FINISHED`].
    fn read_finished(&self, name: &str) -> Result<Option<Vec<u8>>>;
    /// A write of its records, from the version read (62, 63).
    fn write(&self, read_at: u64, changes: sluice_seams::Changes) -> Result<Written>;
    /// Raise its count past `number`, so it gives no number up to it
    /// (design D5; record 0114).
    fn raise_past(&self, number: u64) -> Result<()>;
    /// Its next number (26; record 0114).
    fn next_number(&self) -> Result<u64>;
}

/// What a carry is asked to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owed {
    /// Every live submission of a moving home (design D3).
    All,
    /// Only those the host `me` made, carried into its own files from a
    /// remote home coming home (design D4).
    Mine,
}

/// Record 0122 (design D3 to D5): carry `branch`'s live submissions from
/// the home `from` to the home `to`, under the caller's lock of the branch,
/// then close `from` with its pointer.
///
/// Each submission goes under its number where `to`'s branch holds no
/// submission of that number, live or finished, and under `to`'s next
/// number otherwise, naming the one it had in `was` and the home it came
/// from in `carried_from`; `to`'s count is raised past every number kept.
/// Its owed deliveries and the outcomes still to tell go with it, under
/// the number it has now (core.52). `to` is opened, with `came_from`, where
/// it holds no home or a closed one. Then `from`'s `home.json` is closed
/// naming `to` and each submission carried, and their live documents leave
/// it; its finished submissions, its index and its log stay (design D7).
///
/// The target's write logs `carried` there, with each number as it was and
/// as it is, so the line is written once with what it names.
///
/// Each write reads its target first and writes only what is not there, so
/// a carry stopped between its two writes is finished by the next, which
/// carries nothing twice (59). With [`Owed::Mine`], only what `me` made and
/// `from` has not carried already is carried, and `from` stays closed
/// toward the hosts' own files, listing it among what it carried (D4). A
/// carry that owes nothing writes nothing and answers no home.
pub fn carry(
    branch: &str,
    from: &dyn HomeStore,
    to: &dyn HomeStore,
    me: &str,
    owed: Owed,
    chosen_at: Option<Rev>,
    now: &str,
    after_target: &dyn Fn() -> Result<()>,
) -> Result<Carried> {
    let from_name = from.name();
    let to_name = to.name();
    let mut source = Line::load(branch, from.read()?)
        .with_context(|| format!("reading {branch}'s records in {}", describe(&from_name)))?;
    let already: BTreeSet<u64> = source
        .home
        .as_ref()
        .and_then(|h| h.closed.as_ref())
        .map(|c| c.carried.keys().copied().collect())
        .unwrap_or_default();
    let carrying: Vec<Submission> = source
        .subs
        .values()
        .filter(|s| !already.contains(&s.id))
        .filter(|s| owed == Owed::All || s.from.host.as_deref() == Some(me))
        .cloned()
        .collect();
    let closing = match &source.home {
        Some(h) => h.state != HomeState::Closed,
        None => true,
    };
    if carrying.is_empty() && !closing {
        return Ok(Carried::default());
    }
    let mut target = Line::load(branch, to.read()?)
        .with_context(|| format!("reading {branch}'s records in {}", describe(&to_name)))?;

    // 1. What the target holds already, from a carry that stopped.
    let mut numbers: BTreeMap<u64, u64> = BTreeMap::new();
    let mut fresh: Vec<&Submission> = Vec::new();
    for s in &carrying {
        match holds(&target, s, to)? {
            Some(n) => {
                numbers.insert(s.id, n);
            }
            None => fresh.push(s),
        }
    }

    // 2. Each number kept, the count raised past them, and the rest given
    // the target's next (design D5).
    let (kept, taken): (Vec<&Submission>, Vec<&Submission>) =
        fresh.iter().partition(|s| !target.has(s.id));
    if let Some(top) = kept.iter().map(|s| s.id).max() {
        to.raise_past(top)?;
    }
    let mut given: BTreeMap<u64, u64> = kept.iter().map(|s| (s.id, s.id)).collect();
    for s in &taken {
        given.insert(s.id, to.next_number()?);
    }

    // 3. The target's write.
    if !fresh.is_empty() || target.home.as_ref().is_none_or(|h| !h.is_live()) {
        for s in &fresh {
            let n = given[&s.id];
            let mut carried = (*s).clone();
            carried.id = n;
            if n != s.id {
                carried.was = Some(s.id);
                carried.carried_from = Some(from_name.clone());
            }
            target.subs.insert(n, carried);
        }
        let renumbered: BTreeMap<u64, u64> = given
            .iter()
            .filter(|(old, new)| old != new)
            .map(|(o, n)| (*o, *n))
            .collect();
        let moving: BTreeSet<u64> = fresh.iter().map(|s| s.id).collect();
        let records = carried_deliveries(&source, &target, &moving, owed, &renumbered)?;
        target.add_deliveries(&records);
        for item in source
            .doc
            .outbox
            .iter()
            .filter(|o| moving.contains(&o.submission))
        {
            let n = given[&item.submission];
            let item = OutboxItem {
                id: rekey(&item.id, n),
                submission: n,
                outcome: item.outcome.clone(),
            };
            if !target.doc.outbox.iter().any(|o| o.id == item.id) {
                target.doc.outbox.push(item);
            }
        }
        if target.home.as_ref().is_none_or(|h| !h.is_live()) {
            // A home opened by this carry, or opened again: no lead or
            // keeping of before stands in it (72).
            target.doc.lead = None;
            target.doc.kept = None;
            let mut home = HomeDoc::open(&to_name, chosen_at.clone());
            home.came_from = Some(CameFrom {
                home: from_name.clone(),
                host: Some(me.to_string()),
                carried: given.clone(),
            });
            target.home = Some(home);
        }
        target.log(
            now,
            Event::Carried {
                to: to_name.clone(),
                numbers: given.clone(),
            },
        );
        match to.write(target.version, target.changes())? {
            Written::Written(_) => {}
            Written::Refused => bail!(
                "{branch}'s records in {} were written while its queue was carried there; the next round carries it",
                describe(&to_name)
            ),
        }
    }
    numbers.extend(given);
    after_target()?;

    // 4. The source closed with its pointer, its carried documents gone.
    let mut home = source
        .home
        .clone()
        .unwrap_or_else(|| HomeDoc::open(&from_name, None));
    let mut closed = home.closed.clone().unwrap_or(Closed {
        to: to_name.clone(),
        carried: BTreeMap::new(),
    });
    if home.state != HomeState::Closed {
        closed.to = to_name.clone();
    }
    let host = (to_name == HERE).then(|| me.to_string());
    for (old, n) in &numbers {
        closed.carried.insert(
            *old,
            CarriedTo {
                number: *n,
                host: host.clone(),
            },
        );
    }
    home.state = HomeState::Closed;
    home.moving = None;
    home.closed = Some(closed);
    source.home = Some(home);
    for old in numbers.keys() {
        source.subs.remove(old);
    }
    let mut changes = source.changes();
    for old in numbers.keys() {
        changes.remove.insert(crate::records::submission_file(*old));
    }
    match from.write(source.version, changes)? {
        Written::Written(_) => {}
        Written::Refused => bail!(
            "{branch}'s records in {} were written while its queue was carried from there; the next round closes it",
            describe(&from_name)
        ),
    }
    Ok(Carried {
        to: Some(to_name),
        numbers,
    })
}

/// The number `target` holds `s` under, carried by an earlier carry that
/// stopped: among its live submissions, or among its finished ones from the
/// same work, which a round there may have landed since.
fn holds(target: &Line, s: &Submission, to: &dyn HomeStore) -> Result<Option<u64>> {
    if let Some(found) = target.subs.values().find(|t| same_work(t, s)) {
        return Ok(Some(found.id));
    }
    let maybe: Vec<&FinishedLine> = target
        .finished
        .values()
        .filter(|f| f.worktree == s.from.worktree)
        .collect();
    for f in maybe {
        let Some(bytes) = to.read_finished(&finished_name(f.id))? else {
            continue;
        };
        let done: Submission = serde_json::from_slice(&bytes)?;
        if same_work(&done, s) {
            return Ok(Some(done.id));
        }
    }
    Ok(None)
}

/// The deliveries a carry takes to `target` (core.52): for a whole home,
/// every record `source` holds; coming home, those of the submissions it
/// carries and the readers' known names. Each record under the key of a
/// submission carried under another number is carried under that number's
/// key, and no record `target` holds already is taken twice.
fn carried_deliveries(
    source: &Line,
    target: &Line,
    moving: &BTreeSet<u64>,
    owed: Owed,
    renumbered: &BTreeMap<u64, u64>,
) -> Result<Vec<weir_delivery::Record>> {
    let mut out = Vec::new();
    for record in source.deliveries() {
        let of = key_of(record).and_then(crate::line::submission_of);
        let takes = match (owed, of) {
            (Owed::All, _) => true,
            (Owed::Mine, None) => true,
            (Owed::Mine, Some(id)) => moving.contains(&id),
        };
        if !takes {
            continue;
        }
        let record = match of.and_then(|id| renumbered.get(&id)) {
            Some(n) => rekeyed(record, *n)?,
            None => record.clone(),
        };
        if !target.deliveries().contains(&record) && !out.contains(&record) {
            out.push(record);
        }
    }
    Ok(out)
}

fn key_of(record: &weir_delivery::Record) -> Option<&str> {
    match record {
        weir_delivery::Record::Delivery(d) => Some(d.key().as_str()),
        weir_delivery::Record::Handover(h) => Some(h.key.as_str()),
        weir_delivery::Record::Confirmation(c) => Some(c.key.as_str()),
        weir_delivery::Record::Known(_) => None,
    }
}

/// `key`, `<id>:<outcome>`, naming `n` in place of its submission.
fn rekey(key: &str, n: u64) -> String {
    match key.split_once(':') {
        Some((_, outcome)) => format!("{n}:{outcome}"),
        None => key.to_string(),
    }
}

/// `record` under the key naming `n`, as weir's own spelling writes and
/// reads it, every other field as written.
fn rekeyed(record: &weir_delivery::Record, n: u64) -> Result<weir_delivery::Record> {
    let mut v: serde_json::Value = serde_json::from_str(&weir_delivery::json::write(record))?;
    let key = v["key"]
        .as_str()
        .ok_or_else(|| anyhow!("a delivery's record with no key"))?
        .to_string();
    v["key"] = serde_json::Value::String(rekey(&key, n));
    weir_delivery::json::read(&serde_json::to_string(&v)?).map_err(|refused| anyhow!("{refused}"))
}
