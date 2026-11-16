//! A run's results as an in-toto statement (v1): one subject per check
//! and system, named `<check>@<system>` with the task runner's key as its
//! digest, and a predicate holding each outcome and the claims the
//! signer makes of the run (79; C.6).

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::{Network, Outcome, Rev};

use super::dsse::{Envelope, IN_TOTO};

pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";

/// What a run's predicate is.
pub const RUN_PREDICATE: &str = "https://agentplot.github.io/sluice/run/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    #[serde(rename = "_type")]
    pub type_: String,
    pub subject: Vec<Subject>,
    pub predicate_type: String,
    pub predicate: Run,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    pub name: String,
    pub digest: BTreeMap<String, String>,
}

/// The run's predicate: what it observed and what its signer claims of
/// it (16, 79, 85).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub results: Vec<RunResult>,
    pub claims: Claims,
    pub branch: String,
    pub combination: Rev,
    /// The claim it ran under (75); `None` for a run on a pull request's
    /// head (113) or a session's (89).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
    /// When it finished, RFC 3339, as the signer's host read its clock.
    pub at: String,
    /// What the executor held the run to beyond its claims (19, 83, 138;
    /// design D3); `None` on a statement signed before it, whose results
    /// read with no limits and no grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
}

/// 19, 83, 138 (design D3): what the executor held a run to beyond the
/// claims, each absent where it held none: its memory, network, container
/// engine and grant. It is what the executor observed, as the system is,
/// so it needs no allowance of the signer's, and a signed result's limits
/// and grant are read from it alone (rule core.22). One statement holds
/// one executor's limits and grant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The most memory the run could use, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    /// The network the executor held the run to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
    /// The run could reach the host's container engine (ADR 0016).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub docker: bool,
    /// What the executor let the run reach (138; record 0072).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    pub check: String,
    pub key: String,
    pub outcome: Outcome,
    /// Answered from the task runner's cache, not run: no observation of
    /// the signer's, which counts only through a pass signed for its key
    /// (108; rule core.9).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
    /// Clause 119: how long the task runner says it
    /// took, in milliseconds, and how often it tried it. Shown with the
    /// result; no verifier or verdict reads them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
}

/// 79: what the signer claims of the run; each counts only when the
/// policy lets the signer make it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// The executor's kind (C.2).
    pub executor: String,
    pub confined: bool,
    pub system: String,
}

impl Statement {
    /// The statement of one run's results on `claims.system`.
    pub fn of(run: Run) -> Statement {
        let subject = run
            .results
            .iter()
            .map(|r| Subject {
                name: format!("{}@{}", r.check, run.claims.system),
                digest: BTreeMap::from([("sha256".to_string(), r.key.clone())]),
            })
            .collect();
        Statement {
            type_: STATEMENT_TYPE.into(),
            subject,
            predicate_type: RUN_PREDICATE.into(),
            predicate: run,
        }
    }

    /// An unsigned envelope of the statement.
    pub fn envelope(&self) -> Result<Envelope> {
        Ok(Envelope::new(IN_TOTO, &serde_json::to_vec(self)?))
    }

    /// The statement an envelope holds, checked for being a run's: its
    /// subjects exactly its results on its system, so neither can say what
    /// the other does not.
    pub fn from_envelope(e: &Envelope) -> Result<Statement> {
        let body = e.payload_of(IN_TOTO)?;
        let s: Statement =
            serde_json::from_slice(&body).context("the envelope holds no in-toto statement")?;
        Statement::checked(s)
    }

    /// `s`, checked for being a run's statement as [`Statement::from_envelope`]
    /// checks an envelope's: a keyless bundle's statement is held to the
    /// same (record 0119; design D13).
    pub fn checked(s: Statement) -> Result<Statement> {
        if s.type_ != STATEMENT_TYPE {
            bail!("the statement is {}, not {STATEMENT_TYPE}", s.type_);
        }
        if s.predicate_type != RUN_PREDICATE {
            bail!(
                "the statement's predicate is {}, not a run",
                s.predicate_type
            );
        }
        if Statement::of(s.predicate.clone()).subject != s.subject {
            bail!("the statement's subjects are not its results");
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn run() -> Run {
        Run {
            results: vec![
                RunResult {
                    check: "cli:build".into(),
                    key: "6de6b709".into(),
                    outcome: Outcome::Pass,
                    replayed: false,
                    took_ms: Some(2_000),
                    attempts: Some(2),
                },
                RunResult {
                    check: "cli:test".into(),
                    key: "4863a9af".into(),
                    outcome: Outcome::Fail,
                    replayed: false,
                    took_ms: None,
                    attempts: None,
                },
            ],
            claims: Claims {
                executor: "container".into(),
                confined: true,
                system: "linux-x64".into(),
            },
            branch: "main".into(),
            combination: Rev::new("c0ffee"),
            claim: Some("main/c3/linux-x64".into()),
            at: "2026-10-19T12:00:00Z".into(),
            held: None,
        }
    }

    /// 138, design D3 (the check-grant spec's "An older statement"): a
    /// statement signed with no limits and no grant reads with none and is
    /// written without them; one with limits and a grant reads them back.
    #[test]
    fn a_statement_with_no_grant_reads_with_none() {
        let s = Statement::of(run());
        let json = serde_json::to_value(&s).unwrap();
        assert!(json["predicate"].get("held").is_none(), "{json}");
        let read = Statement::from_envelope(&s.envelope().unwrap()).unwrap();
        assert_eq!(read.predicate.held, None);
        assert_eq!(serde_json::to_value(&read).unwrap(), json);

        let held = Held {
            memory: Some(8 << 30),
            network: Some(Network::Localhost),
            docker: true,
            grant: Some("sb1:0123456789abcdef0123456789abcdef".into()),
        };
        let s = Statement::of(Run {
            held: Some(held.clone()),
            ..run()
        });
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["predicate"]["held"]["network"], "localhost");
        assert_eq!(json["predicate"]["held"]["docker"], true);
        let read = Statement::from_envelope(&s.envelope().unwrap()).unwrap();
        assert_eq!(read.predicate.held, Some(held));

        let bare = Statement::of(Run {
            held: Some(Held::default()),
            ..run()
        });
        let json = serde_json::to_value(&bare).unwrap();
        assert_eq!(json["predicate"]["held"], serde_json::json!({}));
    }

    #[test]
    fn a_runs_statement_names_each_check_on_its_system() {
        let s = Statement::of(run());
        assert_eq!(s.subject[0].name, "cli:build@linux-x64");
        assert_eq!(s.subject[0].digest["sha256"], "6de6b709");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["_type"], STATEMENT_TYPE);
        assert_eq!(json["predicateType"], RUN_PREDICATE);
        assert_eq!(json["predicate"]["claims"]["system"], "linux-x64");
        let back = Statement::from_envelope(&s.envelope().unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_runners_detail_travels_signed_and_is_optional() {
        let s = Statement::of(run());
        let back = Statement::from_envelope(&s.envelope().unwrap()).unwrap();
        assert_eq!(back.predicate.results[0].took_ms, Some(2_000));
        assert_eq!(back.predicate.results[0].attempts, Some(2));
        let json = serde_json::to_value(&s).unwrap();
        assert!(json["predicate"]["results"][1].get("took_ms").is_none());
        // A statement signed before the detail existed reads without it.
        let mut old = json.clone();
        old["predicate"]["results"][0]
            .as_object_mut()
            .unwrap()
            .retain(|k, _| k != "took_ms" && k != "attempts");
        let e = Envelope::new(IN_TOTO, &serde_json::to_vec(&old).unwrap());
        let read = Statement::from_envelope(&e).unwrap();
        assert_eq!(read.predicate.results[0].took_ms, None);
    }

    #[test]
    fn subjects_that_say_other_than_the_results_are_refused() {
        let mut s = Statement::of(run());
        s.subject[0].digest.insert("sha256".into(), "beef".into());
        let e = Envelope::new(IN_TOTO, &serde_json::to_vec(&s).unwrap());
        assert!(Statement::from_envelope(&e).is_err());
        let mut s = Statement::of(run());
        s.predicate_type = "https://slsa.dev/provenance/v1".into();
        let e = Envelope::new(IN_TOTO, &serde_json::to_vec(&s).unwrap());
        assert!(Statement::from_envelope(&e).is_err());
    }
}
