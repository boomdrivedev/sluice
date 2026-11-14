//! A branch whose records cannot be read is refused alone: a lookup by
//! number, an amend and a withdrawal go on past it to every branch that
//! reads, and say which branches were not read only when the number is on
//! none of those that were; a round of another branch runs past it, and
//! leaves its places for a later pass (26, rule core.14), over the fakes.
//! A branch whose policy is mended outside sluice is read whole at the
//! revision this host holds, with no round run, and one refused there is
//! refused naming that revision (56, 124, 129; rule core.15).

use sluice_conformance::{policy, previewed, shared, Harness, Rig, BRANCH};
use sluice_core::records::State;
use sluice_core::{
    AmendRefusal, Lookup, Requester, Step, SubmitHead, SubmitRequest, Unread, WithdrawRefusal,
    Withdrew,
};
use sluice_seams::Settings;

/// The branch whose records cannot be read, listed before `bolt/x`.
const REFUSED: &str = "bolt/b";

/// The words its reads are refused with.
const WORDS: &str = "invalid .config/sluice.toml at 4f6fe55f: unknown table [[checkout.cache]]";

/// A submission from `worktree` to `branch`, on its tip.
fn submit_to(h: &Harness, branch: &str, worktree: &str, path: &str) -> u64 {
    let head = h.w().work(&h.w().tip(branch), &[(path, Some("new"))]);
    h.w().session(worktree, &format!("agent-{worktree}"), &head);
    h.sluice()
        .submit(&SubmitRequest {
            head: SubmitHead::Worktree(worktree.into()),
            branch: branch.into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .expect("submit runs")
        .expect("accepted")
        .id
}

/// A bolt and `bolt/b`, one submission queued on each: the harness, and
/// the ids on `bolt/x` and on `bolt/b`.
fn two_queued() -> (Harness, u64, u64) {
    two_queued_under(&policy(false, "fail", 4))
}

/// [`two_queued`], `bolt/b` under `refused`'s policy.
fn two_queued_under(refused: &str) -> (Harness, u64, u64) {
    let h = Harness::bolt();
    h.w().init(
        REFUSED,
        &[(".config/sluice.toml", refused), ("a/lib", "a1")],
    );
    let x = submit_to(&h, BRANCH, "unit/x", "a/x");
    let b = submit_to(&h, REFUSED, "unit/b", "a/b");
    (h, x, b)
}

/// `bolt/b`, as a lookup passed it by.
fn unread() -> Vec<Unread> {
    vec![Unread {
        branch: REFUSED.into(),
        error: WORDS.into(),
    }]
}

/// satisfies: 26, S64
#[test]
fn a_submission_is_found_past_a_branch_that_cannot_be_read() {
    let (h, x, _) = two_queued();
    let gone = submit_to(&h, BRANCH, "unit/y", "a/y");
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    let s = h.sluice();

    assert_eq!(s.state(x).unwrap().map(|s| s.id), Some(x));
    match s.lookup(x).unwrap() {
        Lookup::Found(sub) => assert_eq!((sub.id, sub.branch.as_str()), (x, BRANCH)),
        other => panic!("{x} not found past {REFUSED}: {other:?}"),
    }
    s.amend(x, &Requester::Operator, None, Some(1))
        .unwrap()
        .expect("amended");
    assert_eq!(h.sub(x).priority, 1);
    assert_eq!(
        s.withdraw(gone, &Requester::Operator).unwrap(),
        Ok(Withdrew::Now)
    );

    h.settle();
    assert_eq!(h.sub(x).state, State::Landed);
    assert_eq!(h.sub(gone).state, State::Withdrawn);
}

/// satisfies: 26
#[test]
fn a_submission_on_the_branch_that_cannot_be_read_is_unfound_naming_it() {
    let (h, _, b) = two_queued();
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    let s = h.sluice();

    assert_eq!(s.lookup(b).unwrap(), Lookup::Unfound(unread()));
    assert_eq!(s.state(b).unwrap(), None);
    assert_eq!(
        s.amend(b, &Requester::Operator, None, Some(1)).unwrap(),
        Err(AmendRefusal::NoSuchSubmission(unread()))
    );
    assert_eq!(
        s.withdraw(b, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::NoSuchSubmission(unread()))
    );

    // Read again, it is found where it was.
    h.w().fail_state_reads(REFUSED, None);
    match s.lookup(b).unwrap() {
        Lookup::Found(sub) => assert_eq!((sub.id, sub.branch.as_str()), (b, REFUSED)),
        other => panic!("{b} not found once {REFUSED} reads: {other:?}"),
    }
}

/// satisfies: 26
#[test]
fn a_number_on_no_branch_when_every_branch_reads_names_none() {
    let (h, _, _) = two_queued();
    let s = h.sluice();
    assert_eq!(s.lookup(9999).unwrap(), Lookup::Unfound(vec![]));
    assert_eq!(
        s.amend(9999, &Requester::Operator, None, Some(1)).unwrap(),
        Err(AmendRefusal::NoSuchSubmission(vec![]))
    );
    assert_eq!(
        s.withdraw(9999, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::NoSuchSubmission(vec![]))
    );
}

/// satisfies: 26, 71, rule core.14
#[test]
fn a_round_runs_past_a_branch_that_cannot_be_read() {
    let (h, x, _) = two_queued();
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    let s = h.sluice();
    assert_eq!(s.step(BRANCH).expect("the round runs"), Step::Stepped);
    assert_eq!(h.started().len(), 1, "its run started");
    for _ in 0..16 {
        s.step(BRANCH).expect("the round runs");
    }
    assert_eq!(h.sub(x).state, State::Landed);
}

/// Whether `bolt/b`'s log names `place`.
fn logged_on_refused(h: &Harness, place: &str) -> bool {
    h.w()
        .file(REFUSED, "log.jsonl")
        .is_some_and(|log| log.contains(&format!("\"place\":\"{place}\"")))
}

/// satisfies: 26, 123, rule core.14
#[test]
fn a_removed_worktrees_checkout_on_a_branch_that_cannot_be_read_waits_for_it() {
    let (h, x, _) = two_queued();
    let s = h.sluice();
    s.seams()
        .checkout
        .adopt(
            &h.place("unit/b"),
            REFUSED,
            &Settings::new(),
            "2026-10-28T09:00:00Z",
        )
        .expect("the place is equipped");
    h.remove_worktree("unit/b");
    let gone = s.session_places().unwrap();
    assert!(gone.len() == 1 && gone[0].gone, "{gone:?}");
    let name = gone[0].name.clone();
    h.w().fail_state_reads(REFUSED, Some(WORDS));

    // The rounds of bolt/x run, and keep the checkout on bolt/b.
    h.settle();
    assert_eq!(h.sub(x).state, State::Landed);
    assert_eq!(s.session_places().unwrap(), gone);
    assert_eq!(h.gone_releases_of(&name), 0);

    // The first round after bolt/b reads releases it, and logs it there.
    h.w().fail_state_reads(REFUSED, None);
    h.step();
    assert!(s.session_places().unwrap().is_empty());
    assert_eq!(h.gone_releases_of(&name), 1);
    assert!(logged_on_refused(&h, &name));
}

/// satisfies: 26, previews-1 101, rule core.14
#[test]
fn an_unusable_preview_on_a_branch_that_cannot_be_read_waits_for_it() {
    let (h, x, _) = two_queued_under(&previewed(&policy(false, "fail", 4), "make preview"));
    let s = h.sluice();
    let rev = h.w().tip(REFUSED);
    h.w().preview_fails(true);
    let plan = s.preview_plan(REFUSED, &rev).unwrap().unwrap();
    let (place, made) = s.make_preview(&plan).unwrap();
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    h.w().fail_state_reads(REFUSED, Some(WORDS));

    // The rounds of bolt/x run, and keep the preview on bolt/b.
    h.settle();
    assert_eq!(h.sub(x).state, State::Landed);
    assert!(h.w().preview_releases().is_empty());
    let kept = s.preview_of(&rev).unwrap().expect("kept");
    assert_eq!(kept.failed, None, "not released");

    // The first round after bolt/b reads releases it, and logs it there.
    h.w().fail_state_reads(REFUSED, None);
    h.step();
    assert_eq!(h.w().preview_releases().len(), 1);
    assert!(s.preview_of(&rev).unwrap().unwrap().failed.is_some());
    assert!(logged_on_refused(&h, &made.name));
}

/// satisfies: 26, 56, 124, 129, rule core.15
#[test]
fn a_quiet_branch_mended_outside_sluice_is_read_whole() {
    shared::a_quiet_branch_mended_outside_sluice_is_read_whole(&Harness::bolt());
}

/// satisfies: 26, 56, 124
#[test]
fn status_of_a_submission_on_a_mended_branch_answers_for_it() {
    shared::status_of_a_submission_on_a_mended_branch_answers_for_it(&Harness::bolt());
}

/// satisfies: 26, 56, 124, S64
#[test]
fn a_refusal_names_the_revision_this_host_holds() {
    shared::a_refusal_names_the_revision_this_host_holds(&Harness::bolt());
}

/// satisfies: 26, 129
#[test]
fn a_read_of_a_mended_branch_leaves_its_state_as_it_was() {
    shared::a_read_of_a_mended_branch_leaves_its_state_as_it_was(&Harness::bolt());
}
