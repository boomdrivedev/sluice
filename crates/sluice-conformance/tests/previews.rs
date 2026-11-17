//! Previews: previews-1 clauses 98 to 103, over the fakes.

use sluice_conformance::{policy, previewed, shared, Harness, Rig, BRANCH};
use sluice_core::preview::{PreviewRefusal, PreviewState};
use sluice_core::records::{Event, State};
use sluice_core::Sluice;
use sluice_seams::Setting;

fn harness() -> Harness {
    Harness::new(&previewed(&policy(false, "fail", 4), "make preview"))
}

/// satisfies: previews-1 98, 99, 103, S30, S35
#[test]
fn s30_a_landed_revision_is_previewed_at_its_addresses() {
    shared::s30_a_landed_revision_is_previewed_at_its_addresses(&harness());
}

/// satisfies: previews-1 99, 101, S31
#[test]
fn s31_the_next_landings_preview_replaces_the_last() {
    shared::s31_the_next_landings_preview_replaces_the_last(&harness());
}

/// satisfies: previews-1 100, 101, S32
#[test]
fn s32_asking_again_gives_back_the_same_preview() {
    shared::s32_asking_again_gives_back_the_same_preview(&harness());
}

/// satisfies: previews-1 99, S34: two branches' previews, combinations and
/// a session run under test on one host share no port and no name.
#[test]
fn s34_previews_share_no_port_or_name() {
    let h = harness();
    let s = h.sluice();
    let r1 = h.tip();
    let plan = s.preview_plan(BRANCH, &r1).unwrap().unwrap();
    s.make_preview(&plan).unwrap();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/a");
    h.submit("unit/b");
    h.step();
    h.w().session_run("unit/c", true);
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    let plan = s.preview_plan(BRANCH, &h.tip()).unwrap().unwrap();
    s.make_preview(&plan).unwrap();
    let previews = h.w().live_previews();
    assert_eq!(previews.len(), 2);
    let ports: Vec<&String> = previews.iter().flat_map(|p| p.ports.values()).collect();
    let names: Vec<&String> = previews.iter().flat_map(|p| p.names.values()).collect();
    let unique = |xs: &Vec<&String>| xs.iter().collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(unique(&ports), ports.len());
    assert_eq!(unique(&names), names.len());
}

/// satisfies: previews-1 98, S35
#[test]
fn only_a_landed_revision_has_a_preview() {
    let h = harness();
    let s = h.sluice();
    let head = h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    assert_eq!(
        s.preview_plan(BRANCH, &head).unwrap().unwrap_err(),
        PreviewRefusal::NotLanded
    );
    assert_eq!(
        s.preview_plan("bolt/nope", &h.tip()).unwrap().unwrap_err(),
        PreviewRefusal::NoBranch
    );
    assert!(h.w().live_previews().is_empty());

    let id = h.submit("unit/1");
    h.settle();
    assert_eq!(h.sub(id).state, State::Landed);
    // Landed: its head, the landing, and what the branch held before.
    for rev in [head.clone(), h.tip()] {
        let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
        assert_eq!(plan.revision, rev);
    }
}

/// satisfies: previews-1 98
#[test]
fn a_revision_with_no_preview_table_has_no_preview() {
    let h = Harness::bolt();
    assert_eq!(
        h.sluice()
            .preview_plan(BRANCH, &h.tip())
            .unwrap()
            .unwrap_err(),
        PreviewRefusal::NoTable
    );
}

/// satisfies: previews-1 98; 56
#[test]
fn the_command_is_the_previewed_revisions_and_the_limits_the_branchs() {
    let h = harness();
    let old = h.tip();
    // The branch's policy changes after `old`: a new command and a time
    // limit of its own.
    h.w().hand_commit(
        BRANCH,
        &[(
            ".config/sluice.toml",
            Some(
                &previewed(&policy(false, "fail", 4), "make other")
                    .replace("timeout = \"20m\"", "timeout = \"5m\""),
            ),
        )],
    );
    let s = h.sluice();
    let plan = s.preview_plan(BRANCH, &old).unwrap().unwrap();
    assert_eq!(plan.preview.command, ["/bin/sh", "-c", "make preview"]);
    assert_eq!(plan.timeout, Some(std::time::Duration::from_secs(300)));
    let (place, _) = s.make_preview(&plan).unwrap();
    let request = Sluice::preview_request(&plan, place);
    assert_eq!(request.command.as_deref(), Some("make preview"));
    // 147: the checkout is equipped by the runner the policy names.
    assert_eq!(request.tests["runner"], Setting::Text("fake".into()));
    assert!(!request.tests.contains_key("command"));
    assert!(request.required.is_empty() && !request.replay);
    assert_eq!(request.revision, old);
}

/// satisfies: previews-1 99, 100, S30, S32
#[test]
fn asking_again_gives_the_same_preview() {
    let h = harness();
    let s = h.sluice();
    let rev = h.tip();
    let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
    let (place, made) = s.make_preview(&plan).unwrap();
    assert_eq!(made.name, Sluice::preview_name(&rev));
    // Addresses are known before anything runs (S30).
    assert_eq!(
        made.addresses["web"],
        format!("https://{}.localhost", made.names["SLUICE_NAME_WEB"])
    );
    assert_eq!(
        made.addresses["api"],
        format!("http://localhost:{}/docs", made.ports["SLUICE_PORT_API"])
    );
    assert_eq!(s.preview_state(&made).unwrap(), PreviewState::Waiting);
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    // A caller that crashed asks again (S32): the same preview, no run.
    let again = s.preview_of(&rev).unwrap().unwrap();
    assert_eq!(again.names, made.names);
    assert_eq!(again.ports, made.ports);
    assert!(again.run.is_some());
    let (_, given) = s.make_preview(&plan).unwrap();
    assert_eq!(given, again);
    assert_eq!(h.w().requests().len(), 1);
}

/// satisfies: previews-1 101, 103, S30, S33
#[test]
fn a_preview_comes_up_or_fails_and_decides_nothing() {
    let h = harness();
    let s = h.sluice();
    let rev = h.tip();
    let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
    h.w().hold_runs(true);
    let (place, _) = s.make_preview(&plan).unwrap();
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    let p = s.preview_of(&rev).unwrap().unwrap();
    assert_eq!(s.preview_state(&p).unwrap(), PreviewState::Starting);
    h.w().complete(&rev);
    assert_eq!(s.preview_state(&p).unwrap(), PreviewState::Up);
    assert!(s.release_unusable_previews().unwrap().is_empty());

    // A failing preview of the next landing.
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    let id = h.submit("unit/1");
    h.w().hold_runs(false);
    h.settle();
    assert_eq!(h.sub(id).state, State::Landed);
    let next = h.tip();
    let before = h.w().results_log();
    h.w().preview_fails(true);
    let plan = s.preview_plan(BRANCH, &next).unwrap().unwrap();
    let (place, _) = s.make_preview(&plan).unwrap();
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    let failed = s.preview_of(&next).unwrap().unwrap();
    assert!(matches!(
        s.preview_state(&failed).unwrap(),
        PreviewState::Failed(_)
    ));
    // 103: nothing of it is a result, and the queue never waited on it.
    assert_eq!(h.w().results_log(), before);
    let dropped = s.release_unusable_previews().unwrap();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].name, failed.name);
    assert!(dropped[0].failed.is_some());
    let kept = s.preview_of(&next).unwrap().unwrap();
    assert!(kept.failed.is_some() && kept.ports.is_empty());
    assert!(matches!(
        s.preview_state(&kept).unwrap(),
        PreviewState::Failed(_)
    ));
    assert!(s.release_unusable_previews().unwrap().is_empty());
    // The first, up, was never released by any of it.
    assert_eq!(
        s.preview_state(&s.preview_of(&rev).unwrap().unwrap())
            .unwrap(),
        PreviewState::Up
    );
    assert!(h
        .w()
        .file(BRANCH, "log.jsonl")
        .unwrap()
        .contains(&format!("\"place\":\"{}\"", failed.name)));
    assert!(s.release_preview(&failed.name).unwrap());
    assert!(s.preview_of(&next).unwrap().is_none());
}

/// satisfies: previews-1 101, rule core.16
#[test]
fn a_round_never_releases_a_preview_that_is_up() {
    let h = harness();
    let s = h.sluice();
    let r1 = h.tip();
    let plan = s.preview_plan(BRANCH, &r1).unwrap().unwrap();
    let (place, _) = s.make_preview(&plan).unwrap();
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    for n in 0..2 {
        h.work(
            &format!("unit/{n}"),
            "agent",
            &[("b/x", Some(&n.to_string()))],
        );
        h.submit(&format!("unit/{n}"));
        h.settle();
    }
    assert_ne!(h.tip(), r1);
    let p = s.preview_of(&r1).unwrap().unwrap();
    assert_eq!(s.preview_state(&p).unwrap(), PreviewState::Up);
    assert!(h.w().preview_releases().is_empty());
}

/// satisfies: previews-1 101, 123: a preview whose checkout is gone has
/// the release its run stored run once, is freed and logged `gone` with
/// its release run, and is never released or run again, even when that
/// release failed
#[test]
fn a_preview_whose_checkout_is_gone_is_freed() {
    for fails in [false, true] {
        let h = harness();
        let s = h.sluice();
        // A branch with records, where its log is kept.
        h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
        h.submit("unit/1");
        h.settle();
        let rev = h.tip();
        let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
        let (place, made) = s.make_preview(&plan).unwrap();
        s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
            .unwrap();
        h.w().remove_preview_checkout(&made.name);
        if fails {
            h.w()
                .place_release_fails(&made.name, Some(shared::RELEASE_FAILS));
        }
        let round = s.round(BRANCH).unwrap();
        assert_eq!(round.previews.len(), 1);
        assert_eq!(round.previews[0].failed, None);
        let gone = round.previews[0].gone.clone().expect("it was gone");
        assert!(gone.ran);
        assert_eq!(gone.failed.is_some(), fails, "{gone:?}");
        assert!(s.preview_of(&rev).unwrap().is_none());
        assert!(
            h.w().preview_releases().is_empty(),
            "a release was asked of it"
        );
        assert_eq!(h.w().freed_places(), vec![made.name.clone()]);
        assert_eq!(h.gone_releases_of(&made.name), 1);
        assert!(h.events().iter().any(|e| matches!(
            e,
            Event::Gone { place, ran: true, failed, .. }
                if *place == made.name && failed.is_some() == fails
        )));
        assert!(s.round(BRANCH).unwrap().previews.is_empty());
        assert!(s.previews().unwrap().is_empty());
        assert_eq!(h.gone_releases_of(&made.name), 1);
    }
}

/// satisfies: previews-1 101, 123: a preview whose command failed and
/// whose release fails is kept with what failed and tried again once a
/// round, as a combination's checkout is, then held: listed with its
/// failure, and released only when asked by name
#[test]
fn a_preview_whose_release_fails_is_tried_a_bounded_number_of_times() {
    let h = harness();
    let s = h.sluice();
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.submit("unit/1");
    h.settle();
    let rev = h.tip();
    h.w().preview_fails(true);
    let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
    let (place, made) = s.make_preview(&plan).unwrap();
    s.start_preview(&plan, place, sluice_conformance::shared::session_part(&s))
        .unwrap();
    h.w()
        .place_release_fails(&made.name, Some(shared::RELEASE_FAILS));
    let mut dropped = Vec::new();
    for _ in 0..6 {
        dropped.extend(s.round(BRANCH).unwrap().previews);
    }
    assert_eq!(h.releases_of(&made.name), 4);
    assert_eq!(dropped.len(), 4, "{dropped:?}");
    assert!(dropped.iter().all(|d| d.error.is_some()));
    let held: Vec<bool> = dropped.iter().map(|d| d.held).collect();
    assert_eq!(held, [false, false, false, true]);
    let kept = s.preview_of(&rev).unwrap().unwrap();
    let failure = kept.release_failed.expect("what failed is kept");
    assert!(
        failure.failed.contains(shared::RELEASE_FAILS),
        "{failure:?}"
    );
    assert_eq!((failure.tries, failure.stopped), (4, true));
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::ReleaseFailed { place, tries: 4, held: true, .. } if *place == made.name
    )));

    h.w().place_release_fails(&made.name, None);
    assert!(s.release_preview(&made.name).unwrap());
    assert!(s.preview_of(&rev).unwrap().is_none());
}
