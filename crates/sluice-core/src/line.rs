//! A branch's records, parsed from a snapshot and written back as changes
//! (61, 62; rule core.14).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Context, Result};
use sluice_seams::{Changes, Rev, Snapshot};

use crate::records::{
    finished_file, parse_lines, submission_file, to_document, to_line, BranchDoc, Candidate, Event,
    FinishedLine, HomeDoc, LogLine, OutboxItem, RefusedWrite, State, Submission, BRANCH_FILE,
    DELIVERIES_FILE, FINISHED_INDEX, HOME_FILE, LOG_FILE,
};
use crate::refused_writes::{command_of, ends, stands, STANDING};

#[derive(Clone, Debug)]
pub struct Line {
    pub branch: String,
    pub version: u64,
    pub doc: BranchDoc,
    /// Record 0122 (design D1): the home document, `None` where the
    /// records hold none; written back with the rest where it changed.
    pub home: Option<HomeDoc>,
    /// The submissions in the live records: every one not yet moved to
    /// the finished records.
    pub subs: BTreeMap<u64, Submission>,
    /// The submissions moved to the finished records, as the live index
    /// keeps them (130).
    pub finished: BTreeMap<u64, FinishedLine>,
    /// 51: the branch's deliveries, weir's records as read and then as
    /// added, in the order added (record 0102; design D3).
    deliveries: Vec<weir_delivery::Record>,
    /// How many of `deliveries` were read: the rest this write adds.
    deliveries_read: usize,
    original: BTreeMap<String, Vec<u8>>,
    appended: Vec<u8>,
}

impl Line {
    pub fn load(branch: &str, snapshot: Snapshot) -> Result<Line> {
        let doc = match snapshot.files.get(BRANCH_FILE) {
            Some(bytes) => serde_json::from_slice(bytes)
                .with_context(|| format!("reading {branch}'s {BRANCH_FILE}"))?,
            None => BranchDoc::new(branch),
        };
        let home = match snapshot.files.get(HOME_FILE) {
            Some(bytes) => Some(
                serde_json::from_slice(bytes)
                    .with_context(|| format!("reading {branch}'s {HOME_FILE}"))?,
            ),
            None => None,
        };
        let mut subs = BTreeMap::new();
        for (path, bytes) in &snapshot.files {
            if path.starts_with("submissions/") {
                let s: Submission = serde_json::from_slice(bytes)
                    .with_context(|| format!("reading {branch}'s {path}"))?;
                subs.insert(s.id, s);
            }
        }
        let finished = match snapshot.files.get(FINISHED_INDEX) {
            Some(bytes) => parse_lines::<FinishedLine>(bytes)
                .with_context(|| format!("reading {branch}'s {FINISHED_INDEX}"))?
                .into_iter()
                .map(|f| (f.id, f))
                .collect(),
            None => BTreeMap::new(),
        };
        let deliveries = match snapshot.files.get(DELIVERIES_FILE) {
            Some(bytes) => read_deliveries(bytes)
                .with_context(|| format!("reading {branch}'s {DELIVERIES_FILE}"))?,
            None => Vec::new(),
        };
        Ok(Line {
            branch: branch.into(),
            version: snapshot.version,
            doc,
            home,
            subs,
            finished,
            deliveries_read: deliveries.len(),
            deliveries,
            original: snapshot.files,
            appended: Vec::new(),
        })
    }

    /// 51: the branch's deliveries, in the order added, this write's
    /// after those read (record 0102).
    pub fn deliveries(&self) -> &[weir_delivery::Record] {
        &self.deliveries
    }

    /// 51: hold `records` after the others, in the order given, every one;
    /// nothing is let go of (weir 112, 113; design D3).
    pub fn add_deliveries(&mut self, records: &[weir_delivery::Record]) {
        self.deliveries.extend_from_slice(records);
    }

    /// Whether the branch has had submission `id`, live or finished.
    pub fn has(&self, id: u64) -> bool {
        self.subs.contains_key(&id) || self.finished.contains_key(&id)
    }

    /// 6 to 8: the branch's chain on its revision `base`: what is under
    /// test or judged on it as its combinations hold it, then the rest by
    /// priority and order of submission, each after its `after`; what is
    /// landing kept only where `landing` asks ([`crate::order::chain`]).
    pub fn order(&self, base: Option<&Rev>, landing: bool) -> crate::order::Order {
        let pinned: Vec<Vec<u64>> = self
            .doc
            .candidates
            .iter()
            .filter(|c| Some(&c.on) == base && c.state.pins())
            .map(|c| c.submissions.clone())
            .collect();
        crate::order::chain(self.subs.values(), &self.landed(), &pinned, landing)
    }

    /// Every submission the branch has landed, live or finished: what a
    /// chain's `after` may wait on (7).
    pub fn landed(&self) -> std::collections::BTreeSet<u64> {
        self.subs
            .values()
            .filter(|s| s.state == State::Landed)
            .map(|s| s.id)
            .chain(
                self.finished
                    .values()
                    .filter(|f| f.state == State::Landed)
                    .map(|f| f.id),
            )
            .collect()
    }

    /// How many submissions the branch has had from `worktree`, live or
    /// finished.
    pub fn from_worktree(&self, worktree: &str) -> usize {
        self.subs
            .values()
            .filter(|s| s.from.worktree == worktree)
            .count()
            + self
                .finished
                .values()
                .filter(|f| f.worktree == worktree)
                .count()
    }

    /// Whether submission `s` is done with: landed or withdrawn, with no
    /// outcome still to tell, no landing under way and no combination
    /// that names it. Its records then move to the finished ones.
    fn done_with(&self, s: &Submission) -> bool {
        s.state.is_final()
            && !self.doc.outbox.iter().any(|o| o.submission == s.id)
            && !self
                .doc
                .landing
                .as_ref()
                .is_some_and(|l| l.ids.contains(&s.id))
            && !self
                .doc
                .candidates
                .iter()
                .any(|c| c.submissions.contains(&s.id))
    }

    /// The log as written so far, for a reader (26).
    pub fn read_log(snapshot: &Snapshot) -> Result<Vec<LogLine>> {
        match snapshot.files.get(LOG_FILE) {
            Some(bytes) => parse_lines(bytes),
            None => Ok(Vec::new()),
        }
    }

    /// The revision of the last landing the log records, as read: lines
    /// appended since are not among them.
    pub fn last_landed(&self) -> Result<Option<Rev>> {
        let Some(bytes) = self.original.get(LOG_FILE) else {
            return Ok(None);
        };
        let text = std::str::from_utf8(bytes)?;
        for l in text.lines().rev().filter(|l| !l.trim().is_empty()) {
            let line: LogLine = serde_json::from_str(l)
                .with_context(|| format!("reading {}'s {LOG_FILE}", self.branch))?;
            if let Event::Landed { to, .. } = line.event {
                return Ok(Some(to));
            }
        }
        Ok(None)
    }

    pub fn log(&mut self, at: &str, event: Event) {
        self.appended.extend(to_line(&LogLine {
            schema: crate::records::LOG_SCHEMA.into(),
            at: at.into(),
            event,
        }));
    }

    /// 50, 51: queue `item`, an outcome a round decided at `now`, to be
    /// pushed into its submitter's session, and record its delivery, owed
    /// to the submission's work for `span`: key the item's id, reader the
    /// work as the submission recorded it, a worktree's branch or a
    /// request's `pull/<n>`, and message its notice (weir 100, 101, 108;
    /// record 0102; design D2). Every outcome a round decides is queued
    /// here. A key the branch holds records nothing more, so a round that
    /// runs again records it once. A submission by name has no session and
    /// no worktree to carry it, so nothing is queued and nothing recorded
    /// for it: no driver is asked, no `pending` line is logged, and once
    /// landed it leaves the live records as any finished submission does
    /// (124, 130; rule core.50).
    pub fn push_outcome(
        &mut self,
        item: OutboxItem,
        now: &str,
        span: std::time::Duration,
    ) -> Result<()> {
        let Some(s) = self.subs.get(&item.submission) else {
            self.doc.outbox.push(item);
            return Ok(());
        };
        if s.from.by_name() {
            return Ok(());
        }
        let notice = crate::notice(item.submission, &self.branch, &item.outcome.to_decided());
        let recorded = weir_delivery::record(
            &self.deliveries,
            &item.id,
            &notice,
            &[s.from.worktree.as_str()],
            now,
            span.as_secs(),
        )
        .map_err(|refused| {
            anyhow!(
                "recording #{}'s outcome {} on {}: {refused}",
                item.submission,
                item.id,
                self.branch
            )
        })?;
        if let weir_delivery::Recorded::Add(records) = recorded {
            self.add_deliveries(&records);
        }
        self.doc.outbox.push(item);
        Ok(())
    }

    /// 51: record that submission `id`'s outcome under `key` was handed to
    /// `reader` `by` a way at `now`, reaching `to`, with weir's `hand`, and
    /// its reader's confirmation with it where `confirms`, and log a
    /// `handed` line, and a `confirmed` one where it confirms. An outcome
    /// its reader has confirmed is handed over no more and records nothing;
    /// one the branch holds no delivery of, decided before deliveries were
    /// recorded, is logged `handed` alone (weir 103, 109; design D5, D12).
    /// Whether it recorded the hand-over.
    pub fn hand(
        &mut self,
        id: u64,
        key: &str,
        reader: &str,
        by: &str,
        to: &str,
        confirms: bool,
        now: &str,
    ) -> Result<bool> {
        let recorded = self
            .deliveries
            .iter()
            .any(|r| matches!(r, weir_delivery::Record::Delivery(d) if d.key().as_str() == key));
        if recorded {
            let handed = weir_delivery::hand(&self.deliveries, key, reader, by, now, confirms)
                .map_err(|refused| {
                    anyhow!(
                        "handing #{id}'s outcome {key} to {reader} on {}: {refused}",
                        self.branch
                    )
                })?;
            let weir_delivery::Handed::Add(records) = handed else {
                return Ok(false);
            };
            self.add_deliveries(&records);
        }
        self.log(
            now,
            Event::Handed {
                id,
                key: key.into(),
                by: by.into(),
                to: to.into(),
            },
        );
        if confirms && recorded {
            self.log(
                now,
                Event::Confirmed {
                    id,
                    key: key.into(),
                    reader: reader.into(),
                    by: by.into(),
                },
            );
        }
        Ok(true)
    }

    /// 51, 67: confirm for `reader`, at `now`, submission `id`'s outcome
    /// under `key`, which the worktree that submits again has acted on,
    /// with weir's `confirm`, logged `confirmed` `by` `resubmission`; an
    /// outcome with no delivery, or confirmed already, records nothing
    /// (weir 102; record 0102; design D16).
    pub fn confirm_resubmitted(&mut self, id: u64, key: &str, reader: &str, now: &str) {
        let Ok(weir_delivery::Confirmed::Add(add)) =
            weir_delivery::confirm(&self.deliveries, key, reader, now)
        else {
            return;
        };
        self.add_deliveries(&add);
        self.log(
            now,
            Event::Confirmed {
                id,
                key: key.into(),
                reader: reader.into(),
                by: "resubmission".into(),
            },
        );
    }

    pub fn sub(&self, id: u64) -> Result<&Submission> {
        self.subs
            .get(&id)
            .ok_or_else(|| anyhow!("no submission {id} to {}", self.branch))
    }

    pub fn sub_mut(&mut self, id: u64) -> &mut Submission {
        let branch = &self.branch;
        self.subs
            .get_mut(&id)
            .unwrap_or_else(|| panic!("no submission {id} to {branch}"))
    }

    pub fn candidate(&self, id: &str) -> Option<&Candidate> {
        self.doc.candidates.iter().find(|c| c.id == id)
    }

    pub fn candidate_mut(&mut self, id: &str) -> Option<&mut Candidate> {
        self.doc.candidates.iter_mut().find(|c| c.id == id)
    }

    /// 148: take out every cause of refused writes, of any host, that no
    /// longer stands at `now`, and log the end of each at its last refused
    /// write and a day, in the order they ended; one whose last cannot be
    /// read ends at `now` (design D4).
    pub fn end_refused_writes(&mut self, now: &str) {
        let (standing, mut ended): (Vec<_>, Vec<_>) = std::mem::take(&mut self.doc.refused_writes)
            .into_iter()
            .partition(|c| stands(c, now));
        self.doc.refused_writes = standing;
        ended.sort_by_key(ends);
        for cause in ended {
            let at = ends(&cause).map_or_else(
                || now.to_string(),
                |t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            );
            self.end_refused_write(&at, cause);
        }
    }

    /// 124, 148: a write to `reference` on `host` the protection refused at
    /// `now`, by `by` as git ran it, from `from` to `to`, `None` for a
    /// deletion. Causes past their day are ended first. A standing cause
    /// counts it with no line; any other is logged as `protected` and
    /// stands, putting out the host's cause refused longest ago when it is
    /// past the twenty (design D3; rule core.47).
    pub fn refuse_write(
        &mut self,
        now: &str,
        host: &str,
        reference: &str,
        by: &str,
        from: &Rev,
        to: Option<&Rev>,
    ) {
        self.end_refused_writes(now);
        let command = command_of(by);
        if let Some(cause) = self
            .doc
            .refused_writes
            .iter_mut()
            .find(|c| c.key() == (host, reference, command.as_str()))
        {
            cause.times += 1;
            cause.last = now.into();
            cause.from = from.clone();
            cause.to = to.cloned();
            return;
        }
        self.log(
            now,
            Event::Protected {
                reference: reference.into(),
                from: from.clone(),
                to: to.cloned(),
                by: by.into(),
                host: Some(host.into()),
            },
        );
        self.doc.refused_writes.push(RefusedWrite {
            host: host.into(),
            reference: reference.into(),
            command,
            times: 1,
            first: now.into(),
            last: now.into(),
            from: from.clone(),
            to: to.cloned(),
        });
        let held = self.doc.refused_writes.iter().filter(|c| c.host == host);
        if held.count() > STANDING {
            let oldest = self
                .doc
                .refused_writes
                .iter()
                .enumerate()
                .filter(|(_, c)| c.host == host)
                .min_by_key(|(_, c)| c.last_at())
                .map(|(i, _)| i);
            if let Some(i) = oldest {
                let cause = self.doc.refused_writes.remove(i);
                self.end_refused_write(now, cause);
            }
        }
    }

    fn end_refused_write(&mut self, at: &str, cause: RefusedWrite) {
        self.log(
            at,
            Event::ProtectedEnded {
                host: cause.host,
                reference: cause.reference,
                command: cause.command,
                times: cause.times,
                first: cause.first,
                last: cause.last,
            },
        );
    }

    /// Every file whose bytes changed, and the log lines appended. A write
    /// that changes anything also moves each submission done with to the
    /// finished records, in the same write (130, 63), so records written
    /// before them move on their branch's next write.
    pub fn changes(&self) -> Changes {
        let mut changes = Changes::default();
        let mut index = Vec::new();
        {
            let mut put = |path: String, bytes: Vec<u8>| {
                if self.original.get(&path) != Some(&bytes) {
                    changes.put.insert(path, bytes);
                }
            };
            put(BRANCH_FILE.into(), to_document(&self.doc));
            if let Some(home) = &self.home {
                put(HOME_FILE.into(), to_document(home));
            }
            for s in self.subs.values() {
                if !self.done_with(s) {
                    put(submission_file(s.id), to_document(s));
                }
            }
        }
        // A write that changes nothing moves nothing: it is no write. A
        // submission done with in this very write is a change.
        let added = &self.deliveries[self.deliveries_read..];
        let changed = !changes.put.is_empty()
            || !self.appended.is_empty()
            || !added.is_empty()
            || self.subs.values().any(|s| {
                self.done_with(s)
                    && self.original.get(&submission_file(s.id)) != Some(&to_document(s))
            });
        for s in self.subs.values().filter(|s| changed && self.done_with(s)) {
            changes.put.insert(finished_file(s.id), to_document(s));
            changes.remove.insert(submission_file(s.id));
            index.extend(to_line(&FinishedLine {
                id: s.id,
                state: s.state,
                worktree: s.from.worktree.clone(),
                subject: s.subject.clone(),
            }));
        }
        if !self.appended.is_empty() {
            changes
                .append
                .insert(LOG_FILE.into(), self.appended.clone());
        }
        if !index.is_empty() {
            changes.append.insert(FINISHED_INDEX.into(), index);
        }
        let moving: BTreeSet<u64> = self
            .subs
            .values()
            .filter(|s| changed && self.done_with(s))
            .map(|s| s.id)
            .collect();
        let let_go = match changed {
            true => self.let_go(&moving),
            false => BTreeSet::new(),
        };
        if !let_go.is_empty() {
            let kept: Vec<weir_delivery::Record> = self
                .deliveries
                .iter()
                .filter(|r| key_of(r).is_none_or(|key| !let_go.contains(key)))
                .cloned()
                .collect();
            changes.put.insert(
                DELIVERIES_FILE.into(),
                weir_delivery::json::write_lines(&kept).into_bytes(),
            );
        } else if !added.is_empty() {
            changes.append.insert(
                DELIVERIES_FILE.into(),
                weir_delivery::json::write_lines(added).into_bytes(),
            );
        }
        changes
    }

    /// 51: the keys of the deliveries a write that moves `moving` to the
    /// finished records lets go of: each done, every reader confirmed it
    /// as `owed` reads them (weir 107, 111), whose submission this write
    /// moves, or that was done as read and whose submission has moved
    /// already. A delivery not done stays whatever becomes of its
    /// submission, and a key that names no submission is never let go of
    /// (I9; design D3).
    fn let_go(&self, moving: &BTreeSet<u64>) -> BTreeSet<String> {
        // `owed` reads what is done from the records alone; the time it
        // asks for decides only what has expired.
        let done = |records: &[weir_delivery::Record]| -> BTreeSet<String> {
            weir_delivery::owed(records, "1970-01-01T00:00:00Z")
                .map(|owed| owed.done.iter().map(|k| k.to_string()).collect())
                .unwrap_or_default()
        };
        let was_done = done(&self.deliveries[..self.deliveries_read]);
        done(&self.deliveries)
            .into_iter()
            .filter(|key| {
                let Some(id) = submission_of(key) else {
                    return false;
                };
                moving.contains(&id) || (was_done.contains(key) && !self.subs.contains_key(&id))
            })
            .collect()
    }
}

/// The key a delivery's record names; `None` for a reader's known name,
/// which names none and is never let go of (weir 106).
fn key_of(record: &weir_delivery::Record) -> Option<&str> {
    match record {
        weir_delivery::Record::Delivery(d) => Some(d.key().as_str()),
        weir_delivery::Record::Handover(h) => Some(h.key.as_str()),
        weir_delivery::Record::Confirmation(c) => Some(c.key.as_str()),
        weir_delivery::Record::Known(_) => None,
    }
}

/// 51: the submission an outcome's key `<id>:<outcome>` names (design D2).
pub fn submission_of(key: &str) -> Option<u64> {
    key.split_once(':')?.0.parse().ok()
}

/// 51: a branch's deliveries as `deliveries.jsonl` holds them, one record
/// to a line in C.8's JSON, read by weir's own reader; a line that does
/// not read is refused in weir's words, saying which (design D3).
fn read_deliveries(bytes: &[u8]) -> Result<Vec<weir_delivery::Record>> {
    let text = std::str::from_utf8(bytes)?;
    weir_delivery::json::read_lines(text).map_err(|refused| anyhow!("{refused}"))
}

/// 51, 112: the line is a deliverer's store, its deliveries read in the
/// order added and added to every one or none, letting go of nothing; it
/// is written with the rest of the branch's records, under its lock
/// (weir 113; design D3).
impl weir_delivery::Store for Line {
    type Error = std::convert::Infallible;

    fn read(&self) -> Result<Vec<weir_delivery::Record>, Self::Error> {
        Ok(self.deliveries.clone())
    }

    fn add(&mut self, records: &[weir_delivery::Record]) -> Result<(), Self::Error> {
        self.add_deliveries(records);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landed(id: u64, worktree: &str, subject: Option<&str>) -> Vec<u8> {
        format!(
            "{{\"schema\":\"sluice/submission@1\",\"id\":{id},\"branch\":\"main\",\
             \"head\":\"h{id}\",\"from\":{{\"agent\":\"a\",\"worktree\":\"{worktree}\"}},\
             \"submitted\":\"t\",\"after\":[],\"priority\":0,\"attempt\":1,\
             \"state\":\"landed\"{}}}",
            subject.map_or(String::new(), |s| format!(",\"subject\":{s:?}"))
        )
        .into_bytes()
    }

    /// 26, 130: a submission moved to the finished records keeps its
    /// subject in the live index, and one with none writes no field.
    #[test]
    fn a_finished_submission_keeps_its_subject_in_the_index() {
        let files = [
            (
                submission_file(12),
                landed(12, "unit/review", Some("Batch review cards")),
            ),
            (submission_file(13), landed(13, "unit/forecast", None)),
        ];
        let line = Line::load(
            "main",
            Snapshot {
                version: 1,
                files: files.into_iter().collect(),
            },
        )
        .unwrap();
        let changes = line.changes();
        assert!(changes.remove.contains(&submission_file(12)));
        assert_eq!(
            String::from_utf8(changes.append[FINISHED_INDEX].clone()).unwrap(),
            "{\"id\":12,\"state\":\"landed\",\"worktree\":\"unit/review\",\
             \"subject\":\"Batch review cards\"}\n\
             {\"id\":13,\"state\":\"landed\",\"worktree\":\"unit/forecast\"}\n"
        );
    }

    const DELIVERY: &str = "{\"format\": \"weir-delivery/1\", \"key\": \"12:landed\", \
                            \"readers\": [\"unit-a\"], \
                            \"message\": \"[sluice] #12 landed on main as abc1234.\", \
                            \"recorded_at\": \"2026-11-08T10:02:00Z\", \
                            \"expires_at\": \"2026-11-15T10:02:00Z\"}\n";
    const HANDOVER: &str = "{\"format\": \"weir-handover/1\", \"key\": \"12:landed\", \
                            \"reader\": \"unit-a\", \"by\": \"notice\", \
                            \"at\": \"2026-11-08T10:02:01Z\"}\n";

    fn with_deliveries(text: &str) -> Result<Line> {
        Line::load(
            "main",
            Snapshot {
                version: 1,
                files: [
                    (
                        BRANCH_FILE.to_string(),
                        to_document(&BranchDoc::new("main")),
                    ),
                    (DELIVERIES_FILE.to_string(), text.as_bytes().to_vec()),
                ]
                .into_iter()
                .collect(),
            },
        )
    }

    /// 51: the branch's deliveries are read in the order written, and a
    /// write appends what it added in C.8's spelling, so the file reads
    /// back as written (record 0102; design D3).
    #[test]
    fn deliveries_read_back_as_written() {
        let mut line = with_deliveries(DELIVERY).unwrap();
        assert_eq!(line.deliveries().len(), 1);
        assert!(line.changes().is_empty(), "a read writes nothing");
        let added = weir_delivery::json::read(HANDOVER.trim_end()).unwrap();
        weir_delivery::Store::add(&mut line, std::slice::from_ref(&added)).unwrap();
        let changes = line.changes();
        assert_eq!(
            String::from_utf8(changes.append[DELIVERIES_FILE].clone()).unwrap(),
            HANDOVER
        );
        assert!(!changes.put.contains_key(DELIVERIES_FILE));
        let both = with_deliveries(&format!("{DELIVERY}{HANDOVER}")).unwrap();
        assert_eq!(
            weir_delivery::Store::read(&both).unwrap(),
            weir_delivery::json::read_lines(&format!("{DELIVERY}{HANDOVER}")).unwrap()
        );
        assert_eq!(
            weir_delivery::json::write_lines(both.deliveries()),
            format!("{DELIVERY}{HANDOVER}")
        );
    }

    /// 51: a line that does not read stops the branch's reads, naming the
    /// file and the field in weir's words (design D3).
    #[test]
    fn deliveries_that_do_not_read_name_the_file() {
        let wrong = DELIVERY.replace("weir-delivery/1", "weir-delivery/2");
        let why = format!("{:#}", with_deliveries(&wrong).unwrap_err());
        assert!(why.contains("main's deliveries.jsonl"), "{why}");
        assert!(why.contains("`format`"), "{why}");
        assert!(why.contains("weir-delivery/2"), "{why}");
    }

    /// A branch's line on the fakes' records seam as a store: each add one
    /// write of the line under the branch's lock, each read the line read
    /// afresh, so what the pack reads back is what the records hold.
    struct OnTheRecords(sluice_fakes::Fakes);

    impl weir_delivery::Store for OnTheRecords {
        type Error = anyhow::Error;

        fn read(&self) -> Result<Vec<weir_delivery::Record>> {
            use sluice_seams::State;
            Ok(Line::load("main", self.0.state.read("main")?)?
                .deliveries()
                .to_vec())
        }

        fn add(&mut self, records: &[weir_delivery::Record]) -> Result<()> {
            use sluice_seams::{State, Written};
            let _held = self
                .0
                .state
                .lock("main")?
                .ok_or_else(|| anyhow!("main's lock is held"))?;
            let mut line = Line::load("main", self.0.state.read("main")?)?;
            line.add_deliveries(records);
            match self.0.state.write("main", line.version, line.changes())? {
                Written::Written(_) => Ok(()),
                Written::Refused => Err(anyhow!("main's records moved under the add")),
            }
        }
    }

    /// 51, 112, 113: the branch's store of deliveries holds every delivery
    /// case of weir's pack, each through a fresh branch's line on the
    /// fakes' records seam (S25; record 0102; design D4).
    #[test]
    fn a_branchs_deliveries_pass_weirs_pack() {
        let verdicts = weir_pack::conform_deliveries(&weir_pack::Pack::embedded(), || {
            OnTheRecords(sluice_fakes::Fakes::new())
        });
        assert!(!verdicts.is_empty(), "the pack holds delivery cases");
        let failed: Vec<_> = verdicts
            .iter()
            .filter_map(|(id, verdict)| verdict.as_ref().err().map(|why| format!("{id}: {why:?}")))
            .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
