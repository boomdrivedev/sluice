//! A reader confirming an outcome by its key, as `sluice confirm` does:
//! 51, S77; weir 102, 104, 106, 110; record 0102.

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::{Collect, ConfirmAs, Confirmation, SubmitHead, SubmitRequest};
use sluice_seams::Driver;

/// The folder of place `worktree`, as a confirm there reads it.
fn folder(h: &Harness, worktree: &str) -> ConfirmAs {
    ConfirmAs::Folder(h.f.driver.session(worktree).unwrap().from)
}

/// `sluice confirm <keys>...` as `who`.
fn confirm(h: &Harness, keys: &[String], who: &ConfirmAs) -> Vec<Confirmation> {
    let got = h.sluice().confirm(keys, who).unwrap();
    assert!(got.unread.is_empty(), "{:?}", got.unread);
    got.answers
}

/// What `sluice inbox` in place `worktree` would hand over, by key.
fn owed_keys(h: &Harness, worktree: &str) -> Vec<String> {
    let me = h.f.driver.session(worktree).unwrap().from;
    let s = h.sluice();
    let owed = s.owed(&me, Collect::All).unwrap();
    owed.outcomes.iter().map(|o| o.key.clone()).collect()
}

/// Whether anything is owed under `key` on `BRANCH`, to any reader, as the
/// branch's status shows it.
fn owed_at_all(h: &Harness, key: &str) -> bool {
    weir_delivery::owed(&h.deliveries(), "2030-01-01T00:00:00Z")
        .unwrap()
        .readers
        .iter()
        .flat_map(|r| &r.deliveries)
        .any(|d| d.delivery.key().as_str() == key)
}

/// The `confirmed` lines of the branch's log, by submission, reader and
/// who confirmed.
fn confirmed_lines(h: &Harness) -> Vec<(u64, String, String)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Confirmed { id, reader, by, .. } => Some((id, reader, by)),
            _ => None,
        })
        .collect()
}

/// A landing from `unit/a`, no carrier taking it, settled. Its submission.
fn landed(h: &Harness) -> u64 {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().not_delivering("agent-a");
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    one
}

/// satisfies: 51; scenario "A landing confirmed"
#[test]
fn a_landing_is_confirmed_by_its_key() {
    let h = Harness::bolt();
    let one = landed(&h);
    let key = format!("{one}:landed");
    assert_eq!(owed_keys(&h, "unit/a"), std::slice::from_ref(&key));

    let answers = confirm(&h, std::slice::from_ref(&key), &folder(&h, "unit/a"));
    assert!(
        matches!(
            &answers[..],
            [Confirmation::Confirmed { key: k, branch, id, reader, .. }]
                if *k == key && branch == BRANCH && *id == one && reader == "unit/a"
        ),
        "{answers:?}"
    );
    assert!(owed_keys(&h, "unit/a").is_empty());
    assert_eq!(h.confirmed(&key), ["unit/a"]);
    assert_eq!(
        confirmed_lines(&h),
        [(one, "unit/a".to_string(), "confirm".to_string())]
    );
    // The reader's name is known now.
    assert!(h
        .deliveries()
        .iter()
        .any(|r| matches!(r, weir_delivery::Record::Known(k) if k.reader.as_str() == "unit/a")));
}

/// satisfies: 51, S77; scenario "An expired blame confirmed"
#[test]
fn an_expired_blame_is_confirmed_as_an_owed_one() {
    let h = Harness::bolt();
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.w().not_delivering("agent-a");
    let blamed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(blamed).state, State::Failed);
    h.w().advance(chrono::Duration::hours(169));
    let key = format!("{blamed}:failed");
    let me = h.f.driver.session("unit/a").unwrap().from;
    let s = h.sluice();
    let owed = s.owed(&me, Collect::All).unwrap();
    assert!(owed.outcomes[0].owed.expired, "past its span");
    drop(owed);
    drop(s);

    let answers = confirm(&h, std::slice::from_ref(&key), &folder(&h, "unit/a"));
    assert!(
        matches!(&answers[..], [Confirmation::Confirmed { .. }]),
        "{answers:?}"
    );
    assert!(!owed_at_all(&h, &key), "nothing owed for it");
}

/// satisfies: 51; scenario "Two keys at once"
#[test]
fn two_keys_are_confirmed_each_on_its_own_branch() {
    // `main` requires only `c:test`; on `bolt/x`, `unit/b`'s work on
    // `a/lib` goes first, so `unit/a`'s conflicts with it there.
    let main = policy(false, "fail", 4) + "\n[branches.main]\nrequired = [\"c:test\"]\n";
    let h = Harness::new(&main);
    h.branch_at("main", &h.tip());
    h.work("unit/b", "agent-b", &[("a/lib", Some("from b"))]);
    h.work("unit/a", "agent-a", &[("a/lib", Some("from a"))]);
    h.w().not_delivering("agent-a");
    h.submit("unit/b");
    let conflict = h.submit("unit/a");
    let landing = h
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
    assert_eq!(h.sub(conflict).state, State::Conflict);
    assert_eq!(h.sub(landing).state, State::Landed);

    let keys = [format!("{landing}:landed"), format!("{conflict}:conflict")];
    let answers = confirm(&h, &keys, &folder(&h, "unit/a"));
    assert!(
        matches!(
            &answers[..],
            [
                Confirmation::Confirmed { branch: first, .. },
                Confirmation::Confirmed { branch: second, .. },
            ] if first == "main" && second == BRANCH
        ),
        "{answers:?}"
    );
    assert!(owed_keys(&h, "unit/a").is_empty());
}

/// satisfies: 51; scenario "Confirmed twice"
#[test]
fn a_second_confirmation_says_when_and_writes_nothing() {
    let h = Harness::bolt();
    let one = landed(&h);
    let key = format!("{one}:landed");
    let first = confirm(&h, std::slice::from_ref(&key), &folder(&h, "unit/a"));
    let Confirmation::Confirmed { at, .. } = &first[0] else {
        panic!("{first:?}");
    };
    let version = h.version();
    let again = confirm(&h, std::slice::from_ref(&key), &folder(&h, "unit/a"));
    assert!(
        matches!(&again[..], [Confirmation::Before { at: when, .. }] if when == at),
        "{again:?}"
    );
    assert_eq!(h.version(), version, "nothing written");
    assert_eq!(confirmed_lines(&h).len(), 1);
}

/// satisfies: 51; scenario "A key never recorded"
#[test]
fn a_key_no_branch_holds_is_not_found() {
    let h = Harness::bolt();
    landed(&h);
    let answers = confirm(&h, &["99:landed".to_string()], &folder(&h, "unit/a"));
    assert_eq!(
        answers,
        [Confirmation::NotFound {
            key: "99:landed".into()
        }]
    );
}

/// satisfies: 51; scenario "Another worktree's blame"
#[test]
fn another_readers_outcome_is_refused_naming_it() {
    let h = Harness::bolt();
    h.fails_with("b:test", "b/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("broken"))]);
    h.w().not_delivering("agent-b");
    let blamed = h.submit("unit/b");
    h.settle();
    assert_eq!(h.sub(blamed).state, State::Failed);

    let key = format!("{blamed}:failed");
    let version = h.version();
    let answers = confirm(&h, std::slice::from_ref(&key), &folder(&h, "unit/a"));
    assert!(
        matches!(
            &answers[..],
            [Confirmation::Another { readers, .. }] if readers == &["unit/b".to_string()]
        ),
        "{answers:?}"
    );
    assert_eq!(h.version(), version, "nothing written");
    assert_eq!(owed_keys(&h, "unit/b"), [key]);
}

/// satisfies: 51, S77; scenario "A removed worktree's blame"
#[test]
fn the_operator_confirms_for_the_outcomes_own_reader() {
    let h = Harness::bolt();
    h.fails_with("b:test", "b/lib", "broken");
    h.work("unit/b", "agent-b", &[("b/lib", Some("broken"))]);
    h.w().not_delivering("agent-b");
    let blamed = h.submit("unit/b");
    h.settle();
    h.remove_worktree("unit/b");

    let key = format!("{blamed}:failed");
    let answers = confirm(&h, std::slice::from_ref(&key), &ConfirmAs::Operator);
    assert!(
        matches!(
            &answers[..],
            [Confirmation::Confirmed { reader, .. }] if reader == "unit/b"
        ),
        "{answers:?}"
    );
    assert!(!owed_at_all(&h, &key));
    assert_eq!(
        confirmed_lines(&h),
        [(blamed, "unit/b".to_string(), "operator".to_string())]
    );
}
