//! A number more than one branch holds names none of them alone: a
//! lookup by it reads every branch, and is refused naming each branch that
//! holds it, in the records' order, and each that could not be read, with
//! nothing done; a number one branch holds is found as before (26; record
//! 0114; rule core.11), over the fakes.

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{State, Submission};
use sluice_core::{
    AmendRefusal, Lookup, Requester, Several, SubmitHead, SubmitRequest, Unread, WithdrawRefusal,
    Withdrew,
};

/// The other branch that holds the number, listed before `bolt/x`.
const OTHER: &str = "bolt/a";

/// A branch whose records cannot be read, listed between the two.
const REFUSED: &str = "bolt/b";

/// The words its reads are refused with.
const WORDS: &str = "invalid .config/sluice.toml at 4f6fe55f: unknown table [[checkout.cache]]";

/// The number both branches hold.
const N: u64 = 26;

/// `branch` made, under the harness's policy.
fn branch(h: &Harness, name: &str) {
    h.w().init(
        name,
        &[
            (".config/sluice.toml", &policy(false, "fail", 4)),
            ("a/lib", "a1"),
        ],
    );
}

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

/// `branch`'s submission `id`, live or finished, from its history.
fn on(h: &Harness, branch: &str, id: u64) -> Submission {
    h.sluice()
        .history(branch)
        .expect("the history reads")
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no submission {id} on {branch}"))
}

/// Submission 26 landed on `bolt/x`, then submission 26 queued on
/// `bolt/a`, as a remote's count and the host's give.
fn landed_and_queued() -> Harness {
    let h = Harness::bolt();
    branch(&h, OTHER);
    h.w().numbers_from(N);
    assert_eq!(submit_to(&h, BRANCH, "unit/x", "a/x"), N);
    h.settle();
    assert_eq!(on(&h, BRANCH, N).state, State::Landed);
    h.w().numbers_from(N);
    assert_eq!(submit_to(&h, OTHER, "unit/a", "a/a"), N);
    h
}

/// Each branch that holds the number, in the records' order.
fn both(unread: Vec<Unread>) -> Several {
    Several {
        branches: vec![OTHER.into(), BRANCH.into()],
        unread,
    }
}

/// satisfies: 26, record 0114, rule core.11
#[test]
fn a_number_two_branches_hold_is_refused_naming_both() {
    let h = landed_and_queued();
    let s = h.sluice();
    let version = (h.w().version(OTHER), h.w().version(BRANCH));

    assert_eq!(s.lookup(N).unwrap(), Lookup::Several(both(vec![])));
    assert_eq!(
        s.withdraw(N, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::Several(both(vec![])))
    );
    assert_eq!(
        s.amend(N, &Requester::Operator, None, Some(5)).unwrap(),
        Err(AmendRefusal::Several(both(vec![])))
    );
    assert_eq!(s.state(N).unwrap(), None);
    assert_eq!(s.landed_as(N).unwrap(), None);

    // Nothing was done to either.
    assert_eq!((h.w().version(OTHER), h.w().version(BRANCH)), version);
    let queued = on(&h, OTHER, N);
    assert_eq!((queued.state, queued.priority), (State::Queued, 0));
    assert_eq!(on(&h, BRANCH, N).state, State::Landed);
}

/// satisfies: 26, record 0114, rule core.11
#[test]
fn a_branch_that_cannot_be_read_is_named_beside_them() {
    let h = landed_and_queued();
    branch(&h, REFUSED);
    submit_to(&h, REFUSED, "unit/b", "a/b");
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    let s = h.sluice();
    let unread = || {
        vec![Unread {
            branch: REFUSED.into(),
            error: WORDS.into(),
        }]
    };

    assert_eq!(s.lookup(N).unwrap(), Lookup::Several(both(unread())));
    assert_eq!(
        s.withdraw(N, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::Several(both(unread())))
    );
    assert_eq!(
        s.amend(N, &Requester::Operator, None, Some(5)).unwrap(),
        Err(AmendRefusal::Several(both(unread())))
    );
    assert_eq!(on(&h, OTHER, N).state, State::Queued);
}

/// satisfies: 26, rule core.11
#[test]
fn a_number_one_branch_holds_is_found_as_before() {
    let h = landed_and_queued();
    let one = submit_to(&h, OTHER, "unit/c", "a/c");
    assert_eq!(one, N + 1);
    let s = h.sluice();

    match s.lookup(one).unwrap() {
        Lookup::Found(sub) => assert_eq!((sub.id, sub.branch.as_str()), (one, OTHER)),
        other => panic!("{one} not found on {OTHER}: {other:?}"),
    }
    assert_eq!(s.state(one).unwrap().map(|s| s.branch), Some(OTHER.into()));
    s.amend(one, &Requester::Operator, None, Some(1))
        .unwrap()
        .expect("amended");
    assert_eq!(
        s.withdraw(one, &Requester::Operator).unwrap(),
        Ok(Withdrew::Now)
    );
    let gone = on(&h, OTHER, one);
    assert_eq!((gone.state, gone.priority), (State::Withdrawn, 1));
    assert_eq!(on(&h, OTHER, N).state, State::Queued);
}

/// satisfies: 26, record 0114, record 0122 (design D5): #41 carried on
/// each of two branches under another number, each record naming `was` 41
/// and the home it came from, is followed on neither alone: the lookup by
/// it is refused naming both, as for any number two branches hold.
#[test]
fn a_number_two_branches_carried_elsewhere_is_refused() {
    use sluice_seams::Changes;
    let h = Harness::bolt();
    branch(&h, OTHER);
    // Each branch holds a submission already, so each lists its records.
    submit_to(&h, BRANCH, "unit/x", "a/x");
    submit_to(&h, OTHER, "unit/a", "a/a");
    for (b, worktree) in [(BRANCH, "unit/y"), (OTHER, "unit/z")] {
        let id = submit_to(&h, b, worktree, "a/carried");
        // As a carry wrote it, #41 having been taken where it went.
        let s = h.sluice();
        let _lock = s.seams().state.lock(b).unwrap().unwrap();
        let read = s.seams().state.read(b).unwrap();
        let file = format!("submissions/{id}.json");
        let mut doc: serde_json::Value = serde_json::from_slice(&read.files[&file]).unwrap();
        doc["was"] = 41.into();
        doc["carried_from"] = "here".into();
        let changes = Changes {
            put: [(file, serde_json::to_vec(&doc).unwrap())].into(),
            ..Changes::default()
        };
        s.seams().state.write(b, read.version, changes).unwrap();
    }
    let s = h.sluice();

    assert_eq!(s.lookup(41).unwrap(), Lookup::Several(both(vec![])));
    assert_eq!(s.moved_to(41).unwrap(), None);
    assert_eq!(s.state(41).unwrap(), None);
}

/// satisfies: 26, record 0114
#[test]
fn the_branch_named_is_read_alone() {
    let h = landed_and_queued();
    branch(&h, REFUSED);
    submit_to(&h, REFUSED, "unit/b", "a/b");
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    let s = h.sluice();

    match s.lookup_on(BRANCH, N).unwrap() {
        Lookup::Found(sub) => {
            assert_eq!((sub.branch.as_str(), sub.state), (BRANCH, State::Landed))
        }
        other => panic!("{BRANCH}'s {N} not found: {other:?}"),
    }
    assert_eq!(
        s.state_on(OTHER, N).unwrap().map(|s| (s.branch, s.state)),
        Some((OTHER.into(), State::Queued))
    );
    assert_eq!(
        s.landed_as_on(BRANCH, N).unwrap(),
        on(&h, BRANCH, N).landed_as
    );
    assert_eq!(s.landed_as_on(OTHER, N).unwrap(), None);

    // Amended and withdrawn on the branch named, the other left as it was.
    s.amend_on(OTHER, N, &Requester::Operator, None, Some(5))
        .unwrap()
        .expect("amended");
    assert_eq!(on(&h, OTHER, N).priority, 5);
    assert_eq!(
        s.withdraw_on(BRANCH, N, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::Final(State::Landed))
    );
    assert_eq!(
        s.withdraw_on(OTHER, N, &Requester::Operator).unwrap(),
        Ok(Withdrew::Now)
    );
    assert_eq!(on(&h, OTHER, N).state, State::Withdrawn);
    assert_eq!(on(&h, BRANCH, N).state, State::Landed);
}

/// satisfies: 26, record 0114, rule core.11
#[test]
fn a_branch_named_that_cannot_be_read_is_named_alone() {
    let h = landed_and_queued();
    branch(&h, REFUSED);
    submit_to(&h, REFUSED, "unit/b", "a/b");
    h.w().fail_state_reads(REFUSED, Some(WORDS));
    h.w()
        .fail_state_reads(OTHER, Some("another branch's words"));
    let s = h.sluice();
    let alone = || {
        vec![Unread {
            branch: REFUSED.into(),
            error: WORDS.into(),
        }]
    };

    assert_eq!(s.lookup_on(REFUSED, N).unwrap(), Lookup::Unfound(alone()));
    assert_eq!(s.state_on(REFUSED, N).unwrap(), None);
    assert_eq!(s.landed_as_on(REFUSED, N).unwrap(), None);
    assert_eq!(
        s.amend_on(REFUSED, N, &Requester::Operator, None, Some(5))
            .unwrap(),
        Err(AmendRefusal::NoSuchSubmission(alone()))
    );
    assert_eq!(
        s.withdraw_on(REFUSED, N, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::NoSuchSubmission(alone()))
    );
}

/// satisfies: 26, record 0114
#[test]
fn a_branch_named_that_holds_no_such_number_holds_none() {
    let h = landed_and_queued();
    branch(&h, REFUSED);
    submit_to(&h, REFUSED, "unit/b", "a/b");
    let s = h.sluice();

    assert_eq!(s.lookup_on(REFUSED, N).unwrap(), Lookup::Unfound(vec![]));
    assert_eq!(s.state_on(REFUSED, N).unwrap(), None);
    assert_eq!(s.landed_as_on(REFUSED, N).unwrap(), None);
    assert_eq!(
        s.amend_on(REFUSED, N, &Requester::Operator, None, Some(5))
            .unwrap(),
        Err(AmendRefusal::NoSuchSubmission(vec![]))
    );
    assert_eq!(
        s.withdraw_on(REFUSED, N, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::NoSuchSubmission(vec![]))
    );
}
