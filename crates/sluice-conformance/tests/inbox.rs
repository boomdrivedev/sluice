//! A session collecting what its submissions are owed, as `sluice inbox`
//! does: each outcome handed over and owed until its reader confirms it:
//! 51, 135; ADR 0068, records 0070, 0102.

use anyhow::{bail, Result};
use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::line::Line;
use sluice_core::records::{Event, State};
use sluice_core::{Collect, Sluice, SubmitHead, SubmitRequest};
use sluice_seams::{Delivery, Driver, Notification, Rev, Session, Submitter, Written};

/// Where a collect in place `worktree` says it reached: the session its
/// driver names, else its worktree's path, else its branch.
fn reached(me: &Submitter) -> String {
    me.session
        .clone()
        .or(me.path.clone())
        .unwrap_or(me.worktree.clone())
}

/// The session in place `worktree` collects what `collect` takes of its
/// owed outcomes and has each handed over to it by `inbox`, confirming on
/// receipt where `confirms`, as `sluice inbox` does. Returns what it
/// collected, by branch and submission.
fn collect_as(h: &Harness, worktree: &str, collect: Collect, confirms: bool) -> Vec<(String, u64)> {
    let s = h.sluice();
    let me = s.seams().driver.session(worktree).unwrap().from;
    let owed = s.owed(&me, collect).unwrap();
    assert!(owed.unread.is_empty(), "{:?}", owed.unread);
    let collected = owed
        .outcomes
        .iter()
        .map(|o| (o.branch.clone(), o.submission.id))
        .collect();
    owed.hand("inbox", &reached(&me), confirms).unwrap();
    collected
}

/// `sluice inbox` in place `worktree`.
fn collect(h: &Harness, worktree: &str) -> Vec<(String, u64)> {
    collect_as(h, worktree, Collect::All, false)
}

/// The `handed` lines of `branch`'s log for submission `id`, by key, way
/// and where each reached.
fn handed_lines(h: &Harness, branch: &str, id: u64) -> Vec<(String, String, String)> {
    let s = h.sluice();
    let snap = s.seams().state.read(branch).unwrap();
    Line::read_log(&snap)
        .unwrap()
        .into_iter()
        .filter_map(|l| match l.event {
            Event::Handed {
                id: handed,
                key,
                by,
                to,
            } if handed == id => Some((key, by, to)),
            _ => None,
        })
        .collect()
}

/// The keys of `branch`'s `handed` lines for submission `id`.
fn handed_keys(h: &Harness, branch: &str, id: u64) -> Vec<String> {
    handed_lines(h, branch, id)
        .into_iter()
        .map(|(key, _, _)| key)
        .collect()
}

/// `reader` confirms the outcome under `key` on `BRANCH`, as `sluice
/// confirm` does.
fn confirm(h: &Harness, key: &str, reader: &str) {
    let s = h.sluice();
    let state = s.seams().state;
    let _lock = state.lock(BRANCH).unwrap().expect("the lock");
    let mut line = Line::load(BRANCH, state.read(BRANCH).unwrap()).unwrap();
    if let weir_delivery::Confirmed::Add(add) =
        weir_delivery::confirm(line.deliveries(), key, reader, "2026-10-11T10:00:00Z").unwrap()
    {
        line.add_deliveries(&add);
    }
    let written = state.write(BRANCH, line.version, line.changes()).unwrap();
    assert!(matches!(written, Written::Written(_)));
}

/// satisfies: 51, 135, S76; scenario "Collecting twice hands over twice"
#[test]
fn each_outcome_collected_is_handed_over_and_stays_owed() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().in_session("unit/a", "pane-1");
    h.w().not_delivering("agent-a");
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let key = format!("{one}:landed");
    assert!(h.handed(&key).is_empty(), "the round handed nothing over");

    // The session comes back in another pane and collects, twice.
    h.w().in_session("unit/a", "pane-2");
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), one)]);
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), one)]);
    assert_eq!(h.handed(&key), ["inbox", "inbox"]);
    assert!(h.confirmed(&key).is_empty());
    assert_eq!(
        handed_lines(&h, BRANCH, one),
        [
            (key.clone(), "inbox".into(), "pane-2".into()),
            (key.clone(), "inbox".into(), "pane-2".into()),
        ]
    );

    // Confirmed, it is collected no more.
    confirm(&h, &key, "unit/a");
    assert!(collect(&h, "unit/a").is_empty());
    assert_eq!(h.handed(&key).len(), 2);
}

/// satisfies: 51, 135, S76; scenario "A session that stopped before it
/// acted collects again"
#[test]
fn an_outcome_the_round_pushed_is_collected_until_it_is_confirmed() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().in_session("unit/a", "pane-1");
    let one = h.submit("unit/a");
    h.settle();
    let key = format!("{one}:landed");
    assert_eq!(h.handed(&key), ["notice"]);
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), one)]);
    assert_eq!(h.handed(&key), ["notice", "inbox"]);
}

/// satisfies: 51, 135; scenario "A caller that keeps its outcomes"
#[test]
fn confirm_confirms_what_the_collect_hands_over() {
    let h = Harness::bolt();
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.w().not_delivering("agent-a");
    let blamed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(blamed).state, State::Failed);

    let key = format!("{blamed}:failed");
    assert_eq!(
        collect_as(&h, "unit/a", Collect::All, true),
        [(BRANCH.to_string(), blamed)]
    );
    assert_eq!(h.handed(&key), ["inbox"]);
    assert_eq!(h.confirmed(&key), ["unit/a"]);
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::Confirmed { id, reader, by, .. }
            if *id == blamed && reader == "unit/a" && by == "inbox"
    )));
    assert!(collect(&h, "unit/a").is_empty());
}

/// satisfies: 51, 135; scenarios "A notice already pushed" and "A new
/// outcome with an older one owed"
#[test]
fn new_hands_over_only_what_no_way_has_handed_over() {
    let h = Harness::bolt();
    h.fails_with("b:test", "b/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().in_session("unit/a", "pane-1");
    let pushed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.handed(&format!("{pushed}:landed")), ["notice"]);
    assert!(
        collect_as(&h, "unit/a", Collect::New, false).is_empty(),
        "the notice was pushed"
    );
    assert_eq!(h.handed(&format!("{pushed}:landed")), ["notice"]);

    // An older one handed over by a collect, and a new one by nothing.
    h.w().not_delivering("agent-a");
    h.work_on("unit/a", &h.tip(), &[("b/lib", Some("broken"))]);
    let blamed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(blamed).state, State::Failed);
    assert_eq!(
        collect_as(&h, "unit/a", Collect::New, false),
        [(BRANCH.to_string(), blamed)]
    );
    assert_eq!(h.handed(&format!("{blamed}:failed")), ["inbox"]);
    assert!(collect_as(&h, "unit/a", Collect::New, false).is_empty());
    assert_eq!(
        collect(&h, "unit/a"),
        [(BRANCH.to_string(), pushed), (BRANCH.to_string(), blamed)],
        "each still owed"
    );
}

/// The fakes' driver, whose next notify fails as sluice's own failure
/// does, delivering nothing: the outbox keeps the outcome for a round.
struct FailsOnce<'a> {
    inner: &'a dyn Driver,
    failed: std::cell::Cell<bool>,
}

impl Driver for FailsOnce<'_> {
    fn session(&self, worktree: &str) -> Result<Session> {
        self.inner.session(worktree)
    }

    fn named(&self, branch: &str) -> Result<Submitter> {
        self.inner.named(branch)
    }

    fn notify(&self, n: &Notification) -> Result<Delivery> {
        if !self.failed.replace(true) {
            bail!("the forge gave up on {}", n.id);
        }
        self.inner.notify(n)
    }
}

/// satisfies: 51, 135
#[test]
fn an_outcome_collected_is_not_pushed_by_the_next_round() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().in_session("unit/a", "pane-1");
    let one = h.submit("unit/a");
    let failing = FailsOnce {
        inner: &h.f.driver,
        failed: Default::default(),
    };
    let seams = sluice_core::Seams {
        driver: &failing,
        ..h.f.seams()
    };
    let landed = (0..20).any(|_| {
        let s = Sluice::new(seams, &h.f.clock);
        let _ = s.step(BRANCH);
        failing.failed.get()
    });
    assert!(landed, "the landing's push failed");
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.doc().outbox.len(), 1, "its outcome is still to be pushed");

    h.w().in_session("unit/a", "pane-2");
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), one)]);
    h.settle();
    assert!(h.doc().outbox.is_empty(), "the round let it go");
    assert!(
        h.w().delivered_to("agent-a").is_empty(),
        "no round pushed it"
    );
    assert_eq!(h.handed(&format!("{one}:landed")), ["inbox"]);
    assert_eq!(
        handed_lines(&h, BRANCH, one),
        [(format!("{one}:landed"), "inbox".into(), "pane-2".into())]
    );
}

/// satisfies: 51, 135
#[test]
fn one_collect_finds_the_outcomes_of_every_branch() {
    // `main` requires only `c:test`, which the work leaves passing;
    // `bolt/x` requires the gate, whose `a:test` it fails.
    let main = policy(false, "fail", 4) + "\n[branches.main]\nrequired = [\"c:test\"]\n";
    let h = Harness::new(&main);
    h.branch_at("main", &h.tip());
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.w().not_delivering("agent-a");
    let blamed = h.submit("unit/a");
    let landed = h
        .sluice()
        .submit(&SubmitRequest {
            head: SubmitHead::Worktree("unit/a".into()),
            branch: "main".into(),
            after: Vec::new(),
            priority: 0,
            subject: None,
            trace: None,
        })
        .unwrap()
        .unwrap()
        .id;
    h.settle();
    for _ in 0..20 {
        h.sluice().step("main").unwrap();
    }
    assert_eq!(h.sub(blamed).state, State::Failed);
    assert_eq!(h.sub(landed).state, State::Landed);

    assert_eq!(
        collect(&h, "unit/a"),
        [(BRANCH.to_string(), blamed), ("main".to_string(), landed)]
    );
    assert_eq!(
        handed_keys(&h, BRANCH, blamed),
        [format!("{blamed}:failed")]
    );
    assert_eq!(
        handed_keys(&h, "main", landed),
        [format!("{landed}:landed")]
    );
    assert!(collect_as(&h, "unit/a", Collect::New, false).is_empty());
}

/// satisfies: 51, 135, 67
#[test]
fn a_failure_withdrawn_by_a_resubmission_is_not_found() {
    let h = Harness::bolt();
    h.fails_with("a:test", "a/lib", "broken");
    let first = h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.w().not_delivering("agent-a");
    let blamed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(blamed).state, State::Failed);

    h.work_on("unit/a", &first, &[("a/lib", Some("fixed"))]);
    let again = h.submit("unit/a");
    assert_eq!(h.sub(blamed).state, State::Withdrawn);
    h.settle();
    assert_eq!(h.sub(again).state, State::Landed);
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), again)]);
    assert!(handed_lines(&h, BRANCH, blamed).is_empty());
}

/// The fakes' driver as a driver with no carrier for a submitter that
/// recorded no session: each such notify is kept and answered undelivered,
/// as the worktrunk driver answers a submit made in no pane.
struct NoSession<'a> {
    inner: &'a dyn Driver,
    notified: std::cell::RefCell<Vec<Notification>>,
}

impl Driver for NoSession<'_> {
    fn session(&self, worktree: &str) -> Result<Session> {
        self.inner.session(worktree)
    }

    fn named(&self, branch: &str) -> Result<Submitter> {
        self.inner.named(branch)
    }

    fn notify(&self, n: &Notification) -> Result<Delivery> {
        if n.to.session.is_some() {
            return self.inner.notify(n);
        }
        self.notified.borrow_mut().push(n.clone());
        Ok(Delivery::Undelivered {
            why: "no session recorded".into(),
        })
    }
}

/// satisfies: 51, 135
#[test]
fn a_submitter_with_no_session_is_notified_once_and_collects_until_it_confirms() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    assert_eq!(h.sub(one).from.session, None);
    let driver = NoSession {
        inner: &h.f.driver,
        notified: Default::default(),
    };
    let seams = sluice_core::Seams {
        driver: &driver,
        ..h.f.seams()
    };
    for _ in 0..24 {
        Sluice::new(seams, &h.f.clock).step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);

    let notified = driver.notified.borrow().clone();
    assert_eq!(notified.len(), 1, "notified once: {notified:?}");
    assert_eq!(notified[0].submission, one);
    let key = format!("{one}:landed");
    assert!(h.handed(&key).is_empty());
    let pending: Vec<_> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Pending { id, why, .. } => Some((id, why)),
            _ => None,
        })
        .collect();
    assert_eq!(pending, [(one, "no session recorded".to_string())]);
    assert!(h.doc().outbox.is_empty(), "no round tries again");

    // No session to name, so it is handed over to its place, each time
    // until it is confirmed.
    let at = h.sub(one).from.path.clone().unwrap_or("unit/a".into());
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), one)]);
    assert_eq!(
        handed_lines(&h, BRANCH, one),
        [(key.clone(), "inbox".into(), at.clone())]
    );
    assert_eq!(
        collect_as(&h, "unit/a", Collect::All, true),
        [(BRANCH.to_string(), one)]
    );
    assert!(collect(&h, "unit/a").is_empty(), "confirmed");
    for _ in 0..3 {
        Sluice::new(seams, &h.f.clock).step(BRANCH).unwrap();
    }
    assert_eq!(driver.notified.borrow().len(), 1, "notified no more");
    assert!(h.w().delivered_to("agent-a").is_empty());
}

/// The fakes' driver, that also gives intake's session of pull request
/// `pull/9`, opened from the branch `from` at `head`: its branch named as
/// its head, no path and no session (ADR 0025).
struct Requests<'a> {
    inner: &'a dyn Driver,
    from: String,
    head: Rev,
}

impl Driver for Requests<'_> {
    fn session(&self, worktree: &str) -> Result<Session> {
        if worktree != "pull/9" {
            return self.inner.session(worktree);
        }
        Ok(Session {
            head: self.head.clone(),
            from: Submitter {
                agent: "author".into(),
                worktree: worktree.into(),
                path: None,
                head: Some(self.from.clone()),
                host: None,
                session: None,
            },
            dirty: Vec::new(),
            detached: false,
        })
    }

    fn named(&self, branch: &str) -> Result<Submitter> {
        self.inner.named(branch)
    }

    fn notify(&self, n: &Notification) -> Result<Delivery> {
        self.inner.notify(n)
    }
}

/// Pull request `pull/9`, opened from `from` at `head`, taken in as intake
/// takes it, blamed for `a:test` and left pending: its post on the request
/// delivered nothing. Its submission.
fn blamed_request_from(h: &Harness, from: &str, head: &Rev) -> u64 {
    h.fails_with("a:test", "a/lib", "broken");
    h.w().not_delivering("author");
    let driver = Requests {
        inner: &h.f.driver,
        from: from.into(),
        head: head.clone(),
    };
    let seams = sluice_core::Seams {
        driver: &driver,
        ..h.f.seams()
    };
    let id = Sluice::new(seams, &h.f.clock)
        .submit(&SubmitRequest {
            head: SubmitHead::Worktree("pull/9".into()),
            branch: BRANCH.into(),
            after: Vec::new(),
            priority: 0,
            subject: None,
            trace: None,
        })
        .unwrap()
        .unwrap()
        .id;
    h.settle();
    let s = h.sub(id);
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.from.head.as_deref(), Some(from));
    assert!(h.handed(&format!("{id}:failed")).is_empty());
    id
}

/// satisfies: 51, 135; ADR 0025; scenario "A pull request from the
/// worktree's branch is collected"
#[test]
fn a_request_from_the_worktrees_branch_is_collected() {
    let h = Harness::bolt();
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    // `unit/a` is a branch no `[branches]` table names.
    h.branch_at("unit/a", &head);
    let id = blamed_request_from(&h, "unit/a", &head);
    assert_eq!(collect(&h, "unit/a"), [(BRANCH.to_string(), id)]);
    assert_eq!(handed_keys(&h, BRANCH, id), [format!("{id}:failed")]);
    assert_eq!(h.handed(&format!("{id}:failed")), ["inbox"]);
}

/// satisfies: 51, 124, 1; core.50; scenario "A pull request from a branch
/// sluice lands on is no worktree's"
#[test]
fn a_request_from_a_branch_sluice_lands_on_is_no_worktrees() {
    let h = Harness::bolt();
    // `bolt/w` is a branch sluice lands on by the policy's `bolt/*`, and a
    // worktree has checked it out.
    let head = h.w().work(&h.tip(), &[("a/lib", Some("broken"))]);
    h.branch_at("bolt/w", &head);
    h.w().session("bolt/w", "agent-w", &head);
    h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
    let id = blamed_request_from(&h, "bolt/w", &head);
    assert!(collect(&h, "bolt/w").is_empty());
    assert!(collect(&h, "unit/a").is_empty());
    let me = h.f.driver.session("bolt/w").unwrap().from;
    let one = Collect::One {
        branch: BRANCH.into(),
        id,
    };
    assert!(h.sluice().owed(&me, one).unwrap().outcomes.is_empty());
    assert!(h.handed(&format!("{id}:failed")).is_empty());
    assert!(handed_lines(&h, BRANCH, id).is_empty());
}

/// 124: a branch sluice lands on submitted by name, from no worktree,
/// whose outcome no inbox takes (design D6; rule core.50).
mod by_name {
    use sluice_conformance::{Harness, Rig, BRANCH};
    use sluice_core::records::State;
    use sluice_core::Collect;
    use sluice_seams::Driver;

    use super::{collect, handed_lines};

    /// Another bolt, a branch sluice lands on by the policy's `bolt/*`.
    const NAMED: &str = "bolt/w";

    /// `NAMED` submitted by name at a commit applying `changes`, with a
    /// worktree that has checked it out and another on a branch of its
    /// own, and settled. Its submission.
    fn settled_by_name(h: &Harness, changes: &[(&str, Option<&str>)]) -> u64 {
        let at = h.w().work(&h.tip(), changes);
        h.branch_at(NAMED, &at);
        h.w().session(NAMED, "agent-w", &at);
        h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
        let id = h.submit_by_name(NAMED);
        h.settle();
        id
    }

    /// No worktree takes submission `id`, by `sluice inbox` or a wait's
    /// delivery read, and nothing is handed over.
    fn taken_by_none(h: &Harness, id: u64) {
        assert!(collect(h, NAMED).is_empty(), "the worktree on the branch");
        assert!(collect(h, "unit/a").is_empty(), "another worktree");
        for worktree in [NAMED, "unit/a"] {
            let me = h.f.driver.session(worktree).unwrap().from;
            let one = Collect::One {
                branch: BRANCH.into(),
                id,
            };
            assert!(h.sluice().owed(&me, one).unwrap().outcomes.is_empty());
        }
        assert!(handed_lines(h, BRANCH, id).is_empty());
    }

    /// satisfies: 124, 51, 135, core.50; scenario "Blamed, and no inbox
    /// takes it"
    #[test]
    fn a_blamed_submission_by_name_is_collected_by_no_worktree() {
        let h = Harness::bolt();
        h.fails_with("a:test", "a/lib", "broken");
        let id = settled_by_name(&h, &[("a/lib", Some("broken"))]);
        assert_eq!(h.sub(id).state, State::Failed);
        taken_by_none(&h, id);
    }

    /// satisfies: 124, 51, 130, 131, core.50; scenario "A submission by
    /// name is no worktree's": landed, and read from the finished records.
    #[test]
    fn a_landed_submission_by_name_is_collected_by_no_worktree() {
        let h = Harness::bolt();
        let id = settled_by_name(&h, &[("a/lib", Some("a2"))]);
        assert_eq!(h.sub(id).state, State::Landed);
        assert!(!h
            .files()
            .contains_key(&sluice_core::records::submission_file(id)));
        taken_by_none(&h, id);

        // The worktree on the branch still collects what it submitted itself.
        let own = h.submit(NAMED);
        h.w().not_delivering("agent-w");
        h.settle();
        assert_eq!(h.sub(own).state, State::Landed);
        assert_eq!(collect(&h, NAMED), [(BRANCH.to_string(), own)]);
    }
}
