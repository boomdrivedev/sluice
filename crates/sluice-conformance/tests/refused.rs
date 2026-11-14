//! A read of the branch's remote a round could not make: the round forms,
//! starts and lands nothing and says why, and the read is logged on the
//! branch once per cause and host, with its end, kept on the host until
//! the records can be written (133), over the fakes.

use sluice_conformance::{Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::refused::Refusal;
use sluice_core::{Peer, Step};
use sluice_seams::read_refused;

const NOT_FOUND: &str = "remote: Repository not found.\n\
                         fatal: repository 'https://github.com/acme/app.git/' not found";

const UNAVAILABLE: &str = "fatal: unable to access 'https://github.com/acme/app.git/': \
                           The requested URL returned error: 503";

/// The name the harness's sluice logs its own lines under.
const HOST: &str = "this-host";

/// A bolt with one submission queued: the harness and its id.
fn queued() -> (Harness, u64) {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let id = h.submit("unit/a");
    (h, id)
}

/// One round on `host`, as that host's own sluice.
fn round_on(h: &Harness, host: &str) -> anyhow::Result<Step> {
    h.w().be_host(host);
    let peer = Peer {
        name: host.into(),
        ..Peer::default()
    };
    h.sluice().as_peer(peer).step(BRANCH)
}

/// One round on the harness's own host.
fn round(h: &Harness) -> anyhow::Result<Step> {
    h.sluice().step(BRANCH)
}

/// The log's `read-refused` and `read-again` lines, each with its time.
fn refusal_lines(h: &Harness) -> Vec<(String, Event)> {
    h.log()
        .into_iter()
        .filter(|l| matches!(l.event, Event::ReadRefused { .. } | Event::ReadAgain { .. }))
        .map(|l| (l.at, l.event))
        .collect()
}

/// The `read-refused` line of `host` for `asked`, refused with `said`.
fn refused(host: &str, asked: &str, said: &str) -> Event {
    Event::ReadRefused {
        host: host.into(),
        remote: "origin".into(),
        asked: asked.into(),
        said: said.into(),
        intake: false,
    }
}

fn branch_ref() -> String {
    format!("refs/heads/{BRANCH}")
}

fn state_ref() -> String {
    format!("refs/sluice/state/{BRANCH}")
}

/// The fake world's host's refusal, as it keeps it.
fn kept(h: &Harness) -> Option<Refusal> {
    h.w()
        .refusal(BRANCH)
        .map(|d| Refusal::parse(d.as_bytes()).expect("the refusal parses"))
}

/// satisfies: 133
#[test]
fn a_refused_round_forms_starts_and_lands_nothing_and_says_why() {
    let (h, id) = queued();
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    let err = round(&h).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("fetching {BRANCH} from origin failed: {NOT_FOUND}"),
        "the round's word to whoever ran it is today's"
    );
    assert_eq!(read_refused(&err).unwrap().asked, branch_ref());
    assert!(h.started().is_empty());
    assert!(h.made_places().is_empty());
    assert_eq!(h.moves(), 0);
    assert_eq!(h.sub(id).state, State::Queued);
    let lines = refusal_lines(&h);
    assert_eq!(
        lines.iter().map(|(_, e)| e).collect::<Vec<_>>(),
        [&refused(HOST, &branch_ref(), NOT_FOUND)],
        "the records are on the host, so the round writes its line before it fails"
    );
    h.w().refuse_reads(BRANCH, None);
    h.settle();
    assert_eq!(h.sub(id).state, State::Landed);
}

/// satisfies: 133, 80
#[test]
fn a_round_whose_state_read_was_answered_stale_does_nothing() {
    let (h, id) = queued();
    let version = h.version();
    h.w().refuse_state_reads(BRANCH, Some(UNAVAILABLE));
    let err = round(&h).unwrap_err();
    assert_eq!(read_refused(&err).unwrap().asked, state_ref());
    assert!(
        err.to_string()
            .starts_with(&format!("fetching {} from origin failed", state_ref())),
        "{err:#}"
    );
    assert!(h.started().is_empty());
    assert_eq!(h.moves(), 0);
    assert_eq!(h.version(), version, "nothing is written");
    let status = h.sluice().status(BRANCH).unwrap();
    assert_eq!(status.stale.as_deref(), Some("origin"));
    assert_eq!(status.submissions[0].id, id);
}

/// satisfies: 133
#[test]
fn a_busy_round_or_another_error_counts_as_no_round() {
    let (h, _) = queued();
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    assert!(round(&h).is_err());
    h.w().hold_lock(BRANCH, true);
    assert_eq!(round(&h).unwrap(), Step::Busy);
    h.w().hold_lock(BRANCH, false);
    assert_eq!(kept(&h).unwrap().rounds, 1);
    assert!(round(&h).is_err());
    assert_eq!(kept(&h).unwrap().rounds, 2);
    // A round that read the remote and failed otherwise neither counts
    // nor ends the refusal.
    h.w().refuse_reads(BRANCH, None);
    h.w().crash_next_write();
    let err = round(&h).unwrap_err();
    assert!(read_refused(&err).is_none(), "{err:#}");
    let r = kept(&h).unwrap();
    assert_eq!((r.rounds, r.ended), (2, None));
}

/// satisfies: 133
#[test]
fn forty_rounds_refused_alike_log_one_line() {
    let (h, _) = queued();
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    for _ in 0..40 {
        assert!(round(&h).is_err());
    }
    assert_eq!(refusal_lines(&h).len(), 1);
    assert_eq!(kept(&h).unwrap().rounds, 40);
}

/// satisfies: 133
#[test]
fn words_that_change_and_change_back_log_one_line_each() {
    let (h, _) = queued();
    for said in [NOT_FOUND, UNAVAILABLE, NOT_FOUND] {
        h.w().refuse_reads(BRANCH, Some(said));
        for _ in 0..3 {
            assert!(round(&h).is_err());
        }
    }
    let lines: Vec<Event> = refusal_lines(&h).into_iter().map(|(_, e)| e).collect();
    assert_eq!(
        lines,
        [
            refused(HOST, &branch_ref(), NOT_FOUND),
            refused(HOST, &branch_ref(), UNAVAILABLE),
        ]
    );
    assert_eq!(kept(&h).unwrap().rounds, 9);
}

/// satisfies: 133
#[test]
fn two_hosts_refused_log_their_own() {
    let (h, _) = queued();
    for host in ["build-1", "build-2"] {
        h.w().be_host(host);
        h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    }
    for _ in 0..3 {
        for host in ["build-1", "build-2"] {
            assert!(round_on(&h, host).is_err());
        }
    }
    let lines: Vec<Event> = refusal_lines(&h).into_iter().map(|(_, e)| e).collect();
    assert_eq!(
        lines,
        [
            refused("build-1", &branch_ref(), NOT_FOUND),
            refused("build-2", &branch_ref(), NOT_FOUND),
        ]
    );
    for host in ["build-1", "build-2"] {
        h.w().be_host(host);
        assert_eq!(kept(&h).unwrap().rounds, 3);
    }
}

/// satisfies: 133
#[test]
fn the_end_is_logged_with_how_long_and_how_many_rounds() {
    let (h, _) = queued();
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    for _ in 0..12 {
        assert!(round(&h).is_err());
        h.w().advance(chrono::Duration::seconds(150));
    }
    h.w().refuse_reads(BRANCH, None);
    round(&h).unwrap();
    let lines = refusal_lines(&h);
    let [(began, first), (ended, end)] = &lines[..] else {
        panic!("{lines:?}");
    };
    assert_eq!(*first, refused(HOST, &branch_ref(), NOT_FOUND));
    let stood = chrono::DateTime::parse_from_rfc3339(ended).unwrap()
        - chrono::DateTime::parse_from_rfc3339(began).unwrap();
    assert!(stood.num_seconds() >= 1800, "{stood}");
    assert_eq!(
        *end,
        Event::ReadAgain {
            host: HOST.into(),
            remote: "origin".into(),
            since: began.clone(),
            stood: stood.num_seconds() as u64,
            rounds: 12,
            intake: false,
        }
    );
    assert_eq!(kept(&h), None, "nothing is kept once the end is written");
    round(&h).unwrap();
    assert_eq!(refusal_lines(&h).len(), 2, "the end is logged once");
}

/// satisfies: 133
#[test]
fn a_refusal_after_an_end_opens_anew() {
    let (h, _) = queued();
    for rounds in [3, 1] {
        h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
        for _ in 0..rounds {
            assert!(round(&h).is_err());
        }
        h.w().refuse_reads(BRANCH, None);
        round(&h).unwrap();
    }
    let lines: Vec<Event> = refusal_lines(&h).into_iter().map(|(_, e)| e).collect();
    let [opened, ended, again, ended_again] = &lines[..] else {
        panic!("{lines:?}");
    };
    assert_eq!(*opened, refused(HOST, &branch_ref(), NOT_FOUND));
    assert_eq!(again, opened);
    assert!(
        matches!(ended, Event::ReadAgain { rounds: 3, .. }),
        "{ended:?}"
    );
    assert!(
        matches!(ended_again, Event::ReadAgain { rounds: 1, .. }),
        "{ended_again:?}"
    );
}

/// satisfies: 133, 80, 73
#[test]
fn with_the_state_on_the_refusing_remote_both_lines_land_in_the_first_write_after() {
    let (h, id) = queued();
    h.settle();
    assert_eq!(h.sub(id).state, State::Landed);
    let version = h.version();
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    h.w().refuse_state_reads(BRANCH, Some(NOT_FOUND));
    let mut first = None;
    for _ in 0..6 {
        let err = round(&h).unwrap_err();
        assert_eq!(read_refused(&err).unwrap().asked, state_ref());
        first.get_or_insert_with(|| kept(&h).unwrap().since);
        h.w().advance(chrono::Duration::minutes(5));
    }
    assert_eq!(h.version(), version, "the log gains nothing meanwhile");
    h.w().refuse_reads(BRANCH, None);
    h.w().refuse_state_reads(BRANCH, None);
    round(&h).unwrap();
    assert_eq!(h.version(), version + 1, "one write");
    let lines = refusal_lines(&h);
    let [(began, opened), (ended, end)] = &lines[..] else {
        panic!("{lines:?}");
    };
    assert_eq!(Some(began), first.as_ref(), "timed when it happened");
    assert_eq!(*opened, refused(HOST, &state_ref(), NOT_FOUND));
    assert!(ended > began, "{ended} after {began}");
    assert!(
        matches!(end, Event::ReadAgain { rounds: 6, stood, .. } if *stood >= 1800),
        "{end:?}"
    );
    for _ in 0..3 {
        round(&h).unwrap();
    }
    assert_eq!(refusal_lines(&h).len(), 2, "no line is written twice");
    assert_eq!(kept(&h), None);
}

/// The log's `read-refused` and `read-again` lines, each as its event's
/// name and, for an end, how many rounds it counted.
fn refusal_kinds(h: &Harness) -> Vec<(&'static str, u64)> {
    refusal_lines(h)
        .into_iter()
        .map(|(_, e)| match e {
            Event::ReadAgain { rounds, .. } => ("read-again", rounds),
            _ => ("read-refused", 0),
        })
        .collect()
}

/// A landed bolt whose host's refusal of `main` ended after two rounds,
/// its end not yet in the log: the write that would carry it failed.
fn ended_unwritten() -> Harness {
    let (h, id) = queued();
    h.settle();
    assert_eq!(h.sub(id).state, State::Landed);
    h.w().refuse_reads(BRANCH, Some(NOT_FOUND));
    for _ in 0..2 {
        assert!(round(&h).is_err());
        h.w().advance(chrono::Duration::minutes(5));
    }
    h.w().refuse_reads(BRANCH, None);
    h.w().crash_next_write();
    round(&h).unwrap();
    h.w().advance(chrono::Duration::minutes(5));
    let r = kept(&h).unwrap();
    assert!(r.ended.is_some(), "{r:?}");
    assert_eq!(refusal_kinds(&h), [("read-refused", 0)]);
    h
}

/// satisfies: 133, 80, 73
#[test]
fn an_end_whose_write_failed_is_kept_when_the_state_is_then_refused() {
    let h = ended_unwritten();
    h.w().refuse_state_reads(BRANCH, Some(UNAVAILABLE));
    let err = round(&h).unwrap_err();
    assert_eq!(read_refused(&err).unwrap().asked, state_ref());
    let r = kept(&h).unwrap();
    assert_eq!((r.rounds, r.ended, r.earlier.len()), (1, None, 1));
    assert_eq!(r.earlier[0].rounds, 2);
    assert_eq!(
        refusal_kinds(&h).len(),
        1,
        "the log gains nothing meanwhile"
    );
    h.w().advance(chrono::Duration::minutes(5));
    h.w().refuse_state_reads(BRANCH, None);
    round(&h).unwrap();
    assert_eq!(
        refusal_kinds(&h),
        [
            ("read-refused", 0),
            ("read-again", 2),
            ("read-refused", 0),
            ("read-again", 1),
        ],
        "the earlier end is written before the refusal after it"
    );
    let lines = refusal_lines(&h);
    assert_eq!(lines[2].1, refused(HOST, &state_ref(), UNAVAILABLE));
    assert!(
        lines.windows(2).all(|w| w[0].0 <= w[1].0),
        "each at when it happened: {lines:?}"
    );
    assert_eq!(kept(&h), None);
}

/// satisfies: 133
#[test]
fn a_refused_round_is_counted_when_its_write_fails() {
    let h = ended_unwritten();
    h.w().refuse_reads(BRANCH, Some(UNAVAILABLE));
    h.w().crash_next_write();
    assert!(round(&h).is_err());
    let r = kept(&h).unwrap();
    assert_eq!((r.rounds, r.ended, r.earlier.len()), (1, None, 1));
    assert_eq!(refusal_kinds(&h).len(), 1);
    assert!(round(&h).is_err());
    assert_eq!(
        refusal_kinds(&h),
        [("read-refused", 0), ("read-again", 2), ("read-refused", 0)]
    );
    assert_eq!(
        refusal_lines(&h)[2].1,
        refused(HOST, &branch_ref(), UNAVAILABLE)
    );
    let r = kept(&h).unwrap();
    assert_eq!((r.rounds, r.earlier.len()), (2, 0));
    assert!(r.causes.iter().all(|c| c.logged));
}
