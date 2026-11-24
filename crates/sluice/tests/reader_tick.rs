//! What a reader's tick reads and starts, through the binary on a
//! repository on disk with git counting its starts (129). A test binary
//! of its own: a child a test starts holds a copy of every lock the test
//! process holds until it execs, which a test of the fakes in `cli.rs`
//! handing a lock from one holder to another would find taken.

mod support;

use support::OnDisk;

const POLICY: &str =
    "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n[branches.main]\ntimeout = \"1m\"\n";

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// How many times git started for `args`, run once.
fn starts(d: &OnDisk, args: &[&str]) -> usize {
    let before = d.git.count();
    let out = d.sluice(args);
    assert!(out.status.success(), "{args:?}: {}", text(&out));
    d.git.count() - before
}

/// The counting git counts what sluice starts.
#[test]
fn a_counting_git_counts_what_status_starts() {
    let d = OnDisk::new("counted-status", POLICY);
    assert!(starts(&d, &["status", "main"]) > 0, "{:?}", d.git.starts());
}

/// reader-tick, "sluice run in a set-up repository with nothing
/// queued": its passes after the first start no git (129).
#[test]
fn sluice_run_with_nothing_queued_starts_git_at_its_first_pass_alone() {
    let d = OnDisk::new("idle-run", POLICY);
    d.land("a");
    let one = starts(&d, &["run", "--rounds", "1"]);
    let before = d.git.starts().len();
    let six = starts(&d, &["run", "--rounds", "6"]);
    assert_eq!(six, one, "{:?}", &d.git.starts()[before..]);
}

/// refused-landing, "Refused every round" and design D3 of the change
/// `sluice-host-makes-each-change`, on the fakes' paused clock: a branch
/// whose one work is a landing the forge refused has its round, which
/// pushes the landing again, at the first pass and then once a minute,
/// however many passes come between; its passes otherwise read its
/// records alone.
#[test]
fn a_held_landing_is_pushed_once_a_minute() {
    use sluice_core::ops::{SubmitHead, SubmitRequest};
    const BRANCH: &str = "bolt/x";
    let f = sluice_fakes::Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"open\", \"sandbox\", \"container\"]\n",
            ),
            ("lib/a.py", "a"),
        ],
    );
    f.world.task("lib:test", "lib/");
    f.world.forge_refuses(BRANCH, Some("push declined"));
    let head = f
        .world
        .work(&f.world.tip(BRANCH), &[("lib/a.py", Some("b"))]);
    f.world.session("unit/a", "agent-a", &head);
    let host = support::FakeHost::new(f.world.clone(), 0);
    let s = sluice_core::Sluice::new(f.seams(), &f.clock).as_peer(sluice::exec::Host::peer(&host));
    s.submit(&SubmitRequest {
        head: SubmitHead::Worktree("unit/a".into()),
        branch: BRANCH.into(),
        after: vec![],
        priority: 0,
        subject: None,
        trace: None,
    })
    .unwrap()
    .unwrap();
    for _ in 0..10 {
        if s.holds_refused_landing(BRANCH).unwrap() {
            break;
        }
        s.round(BRANCH).unwrap();
    }
    assert!(s.holds_refused_landing(BRANCH).unwrap(), "a landing held");
    let pushes = || f.world.refused_moves().len();
    // The pass's own clock, paused: it moves only as the test moves it.
    struct Paused(std::cell::Cell<chrono::DateTime<chrono::Utc>>);
    impl sluice_core::Clock for Paused {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            self.0.get()
        }
    }
    let clock = Paused(std::cell::Cell::new(chrono::Utc::now()));
    let advance = |secs: i64| clock.0.set(clock.0.get() + chrono::Duration::seconds(secs));
    let mut kept = sluice::host_process::pass::Kept::handed(Default::default(), Default::default());
    let mut pass =
        || sluice::host_process::pass::pass(&s, &host, &(), &clock, false, &mut kept).unwrap();
    // Its combination's checkout released first, each round that releases
    // it in play: two minutes of passes.
    for _ in 0..40 {
        advance(3);
        pass();
    }
    advance(61);
    let before = pushes();
    let report = pass();
    assert!(report.held, "{report:?}");
    assert!(!report.in_play, "{report:?}");
    assert_eq!(pushes(), before + 1, "pushed at the minute");
    for _ in 0..19 {
        advance(3);
        pass();
    }
    assert_eq!(pushes(), before + 1, "no push within the minute");
    advance(3);
    pass();
    assert_eq!(pushes(), before + 2, "pushed again a minute on");
}
