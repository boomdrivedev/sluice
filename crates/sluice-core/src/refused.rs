//! 133: this host's refusal of a branch's remote while it stands: what its
//! rounds could not read, since when and for how many rounds, kept on the
//! host through the records seam and never in the branch's records (design
//! D4). Nothing a round, gate, landing or submission decides reads it (rule
//! core.14): only the next write of the branch's log and `sluice status` do,
//! as 73's observation of a lead is kept.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::ReadRefused;

use crate::records::{to_document, Event};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// The remote its rounds could not read.
    pub remote: String,
    /// When its first round was refused.
    pub since: String,
    /// How many of this host's rounds it refused.
    pub rounds: u64,
    /// Each distinct ref asked for and words said, the cause the latest
    /// round met last.
    pub causes: Vec<Cause>,
    /// When a round read the remote again; `None` while it stands.
    pub ended: Option<String>,
    /// Refusals of this host that ended before it opened, oldest first,
    /// whose lines are not all in the branch's log yet: kept until a write
    /// carries them, so a refusal that opens anew drops none of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<Refusal>,
}

/// One ref asked for and what git said of its refusal (133).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cause {
    pub asked: String,
    pub said: String,
    /// When a round first met it.
    pub at: String,
    /// Its `read-refused` line is in the branch's log.
    pub logged: bool,
}

impl Refusal {
    /// The document as the records seam keeps it.
    pub fn parse(bytes: &[u8]) -> Result<Refusal> {
        serde_json::from_slice(bytes).context("reading this host's refusal of the remote")
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        to_document(self)
    }

    /// `standing`, this host's refusal before a round refused at `now` by
    /// `read`: the round counted, and its cause added when no round met it
    /// before. A refusal that ended opens anew, keeping the ended one among
    /// the earlier until its lines are written.
    pub fn refused(standing: Option<Refusal>, read: &ReadRefused, now: &str) -> Refusal {
        let mut r = match standing {
            Some(r) if r.ended.is_none() => r,
            standing => {
                let mut earlier = Vec::new();
                if let Some(mut ended) = standing {
                    earlier.append(&mut ended.earlier);
                    earlier.push(ended);
                }
                Refusal {
                    remote: read.remote.clone(),
                    since: now.into(),
                    rounds: 0,
                    causes: Vec::new(),
                    ended: None,
                    earlier,
                }
            }
        };
        r.rounds += 1;
        let met = r
            .causes
            .iter()
            .position(|c| c.asked == read.asked && c.said == read.said);
        let cause = match met {
            Some(i) => r.causes.remove(i),
            None => Cause {
                asked: read.asked.clone(),
                said: read.said.clone(),
                at: now.into(),
                logged: false,
            },
        };
        r.causes.push(cause);
        r
    }

    /// A round read the remote again at `now`: the refusal ends there.
    pub fn read_again(&mut self, now: &str) {
        self.ended.get_or_insert_with(|| now.into());
    }

    /// The cause the latest refused round met.
    pub fn latest(&self) -> Option<&Cause> {
        self.causes.last()
    }

    /// Seconds from `since` to `ended`, once it ended.
    pub fn stood(&self) -> Option<u64> {
        let since = chrono::DateTime::parse_from_rfc3339(&self.since).ok()?;
        let ended = chrono::DateTime::parse_from_rfc3339(self.ended.as_deref()?).ok()?;
        Some((ended - since).num_seconds().max(0) as u64)
    }

    /// The lines of it not yet in the branch's log, each with when it
    /// happened, `host`'s, in that order: the earlier refusals' first, then
    /// a `read-refused` for each cause not logged, and a `read-again` once
    /// it ended.
    pub fn unwritten(&self, host: &str) -> Vec<(String, Event)> {
        self.unwritten_as(host, false)
    }

    /// [`Refusal::unwritten`], each line marked `intake` where this is a
    /// refusal of the branch's intake (design D27).
    pub fn unwritten_as(&self, host: &str, intake: bool) -> Vec<(String, Event)> {
        let mut lines: Vec<(String, Event)> = self
            .earlier
            .iter()
            .flat_map(|r| r.unwritten_as(host, intake))
            .collect();
        let mut causes: Vec<&Cause> = self.causes.iter().filter(|c| !c.logged).collect();
        causes.sort_by(|a, b| a.at.cmp(&b.at));
        lines.extend(causes.into_iter().map(|c| {
            (
                c.at.clone(),
                Event::ReadRefused {
                    host: host.into(),
                    remote: self.remote.clone(),
                    asked: c.asked.clone(),
                    said: c.said.clone(),
                    intake,
                },
            )
        }));
        if let Some(ended) = &self.ended {
            lines.push((
                ended.clone(),
                Event::ReadAgain {
                    host: host.into(),
                    remote: self.remote.clone(),
                    since: self.since.clone(),
                    stood: self.stood().unwrap_or(0),
                    rounds: self.rounds,
                    intake,
                },
            ));
        }
        lines
    }

    /// What is kept once [`Refusal::unwritten`]'s lines are written: every
    /// cause logged, no earlier refusal, and nothing once it ended.
    pub fn written(mut self) -> Option<Refusal> {
        if self.ended.is_some() {
            return None;
        }
        self.earlier.clear();
        for c in &mut self.causes {
            c.logged = true;
        }
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(asked: &str, said: &str) -> ReadRefused {
        ReadRefused {
            remote: "origin".into(),
            asked: asked.into(),
            said: said.into(),
            note: Some("origin refused a token made 5s ago".into()),
        }
    }

    const NOT_FOUND: &str = "remote: Repository not found.";
    const UNAVAILABLE: &str = "The requested URL returned error: 503";

    /// 133: every refused round is counted, and a cause is added only for
    /// a ref and words no round of the refusal met before; sluice's note
    /// is in none of it.
    #[test]
    fn a_refused_round_adds_a_cause_only_when_it_is_new() {
        let mut r = None;
        let rounds = [
            ("2026-10-26T09:16:40Z", "refs/heads/main", NOT_FOUND),
            ("2026-10-26T09:17:40Z", "refs/heads/main", NOT_FOUND),
            ("2026-10-26T09:18:40Z", "refs/heads/main", UNAVAILABLE),
            ("2026-10-26T09:19:40Z", "refs/heads/main", NOT_FOUND),
            ("2026-10-26T09:20:40Z", "refs/sluice/state/main", NOT_FOUND),
        ];
        for (at, asked, said) in rounds {
            r = Some(Refusal::refused(r, &read(asked, said), at));
        }
        let r = r.unwrap();
        assert_eq!(r.rounds, 5);
        assert_eq!(r.since, "2026-10-26T09:16:40Z");
        let causes: Vec<(&str, &str, &str)> = r
            .causes
            .iter()
            .map(|c| (c.at.as_str(), c.asked.as_str(), c.said.as_str()))
            .collect();
        assert_eq!(
            causes,
            [
                ("2026-10-26T09:18:40Z", "refs/heads/main", UNAVAILABLE),
                ("2026-10-26T09:16:40Z", "refs/heads/main", NOT_FOUND),
                ("2026-10-26T09:20:40Z", "refs/sluice/state/main", NOT_FOUND),
            ]
        );
        assert_eq!(r.latest().unwrap().asked, "refs/sluice/state/main");
        let lines = r.unwritten("build-1");
        assert_eq!(
            lines.iter().map(|(at, _)| at.as_str()).collect::<Vec<_>>(),
            [
                "2026-10-26T09:16:40Z",
                "2026-10-26T09:18:40Z",
                "2026-10-26T09:20:40Z"
            ],
            "written in the order they happened"
        );
        assert!(!r.to_bytes().windows(5).any(|w| w == b"token"));
    }

    /// 133: once written, a cause is not written again while the refusal
    /// stands, even when a round meets it again; the end is written with
    /// how long it stood, and then nothing is kept; a refusal after it
    /// opens anew.
    #[test]
    fn a_written_line_is_not_written_again_and_the_end_closes_it() {
        let r = Refusal::refused(
            None,
            &read("refs/heads/main", NOT_FOUND),
            "2026-10-26T09:16:40Z",
        );
        let mut r = r.written().unwrap();
        assert!(r.unwritten("build-1").is_empty());
        for _ in 0..11 {
            r = Refusal::refused(
                Some(r),
                &read("refs/heads/main", NOT_FOUND),
                "2026-10-26T09:17:00Z",
            );
        }
        assert!(r.unwritten("build-1").is_empty());
        r.read_again("2026-10-26T09:46:40Z");
        r.read_again("2026-10-26T09:47:40Z");
        assert_eq!(r.stood(), Some(1800));
        assert_eq!(
            r.unwritten("build-1"),
            [(
                "2026-10-26T09:46:40Z".to_string(),
                Event::ReadAgain {
                    host: "build-1".into(),
                    remote: "origin".into(),
                    since: "2026-10-26T09:16:40Z".into(),
                    stood: 1800,
                    rounds: 12,
                    intake: false,
                }
            )]
        );
        assert_eq!(r.written(), None);
        let again = Refusal::refused(
            None,
            &read("refs/heads/main", NOT_FOUND),
            "2026-10-26T10:00:00Z",
        );
        assert_eq!(
            (again.rounds, again.since.as_str()),
            (1, "2026-10-26T10:00:00Z")
        );
        assert_eq!(again.unwritten("build-1").len(), 1);
    }

    /// 133: a refusal that opens anew while the end before it is not yet
    /// in the log keeps that end, and any cause of it not logged, until a
    /// write carries them, first, in the order they happened; once written,
    /// none of them is kept.
    #[test]
    fn a_refusal_opened_anew_keeps_the_unwritten_lines_before_it() {
        let mut first = Refusal::refused(
            None,
            &read("refs/heads/main", NOT_FOUND),
            "2026-10-26T09:16:40Z",
        );
        first.read_again("2026-10-26T09:46:40Z");
        let mut second = Refusal::refused(
            Some(first),
            &read("refs/sluice/state/main", UNAVAILABLE),
            "2026-10-26T10:00:00Z",
        );
        second.read_again("2026-10-26T10:05:00Z");
        let third = Refusal::refused(
            Some(second),
            &read("refs/heads/main", NOT_FOUND),
            "2026-10-26T10:10:00Z",
        );
        assert_eq!(
            (third.rounds, third.since.as_str(), third.earlier.len()),
            (1, "2026-10-26T10:10:00Z", 2)
        );
        let lines: Vec<(String, &str)> = third
            .unwritten("build-1")
            .into_iter()
            .map(|(at, e)| {
                let kind = match e {
                    Event::ReadRefused { .. } => "read-refused",
                    Event::ReadAgain { .. } => "read-again",
                    _ => "other",
                };
                (at[11..].to_string(), kind)
            })
            .collect();
        assert_eq!(
            lines,
            [
                ("09:16:40Z".to_string(), "read-refused"),
                ("09:46:40Z".to_string(), "read-again"),
                ("10:00:00Z".to_string(), "read-refused"),
                ("10:05:00Z".to_string(), "read-again"),
                ("10:10:00Z".to_string(), "read-refused"),
            ]
        );
        let back = Refusal::parse(&third.to_bytes()).unwrap();
        assert_eq!(back, third);
        let kept = third.written().unwrap();
        assert!(kept.earlier.is_empty());
        assert!(kept.unwritten("build-1").is_empty());
    }

    /// The document round-trips as the records seam keeps it.
    #[test]
    fn the_document_round_trips() {
        let r = Refusal::refused(
            None,
            &read("refs/heads/main", NOT_FOUND),
            "2026-10-26T09:16:40Z",
        );
        let v: serde_json::Value = serde_json::from_slice(&r.to_bytes()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "remote": "origin",
                "since": "2026-10-26T09:16:40Z",
                "rounds": 1,
                "causes": [{
                    "asked": "refs/heads/main",
                    "said": NOT_FOUND,
                    "at": "2026-10-26T09:16:40Z",
                    "logged": false,
                }],
                "ended": null,
            })
        );
        assert_eq!(Refusal::parse(&r.to_bytes()).unwrap(), r);
    }
}
