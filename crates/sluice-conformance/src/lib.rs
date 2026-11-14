//! The conformance suite's harness. Every scenario under `tests/` is a
//! `#[test]` whose doc comment names, in a `satisfies:` line, the clauses
//! and scenarios of the statement it exercises.
//!
//! A scenario runs on a [`Rig`]: what every scenario needs of a world, a
//! branch to land on, sessions with work, and what sluice did. The
//! [`Harness`] is the rig over the fakes, which also inject faults
//! (crashes, interference, lost runs, held runs) through its [`World`].
//! The scenarios that need no fault are in [`shared`], and run on the
//! fakes here and on the real profiles in the `sluice` crate's tests.

use std::collections::BTreeMap;

use anyhow::Result;
use sluice_core::line::Line;
use sluice_core::records::{parse_lines, BranchDoc, Event, LogLine, ResultLine, Submission};
use sluice_core::session::Recorded;
use sluice_core::{
    Accepted, GroupRequest, Requester, Sluice, Step, SubmitHead, SubmitRefusal, SubmitRequest,
};
use sluice_fakes::Fakes;
use sluice_seams::{Driver, Notification, Progress, Rev, RunReport, RunRequest};

pub use sluice_fakes::{Crash, World};

pub mod shared;

/// The branch every scenario lands on, unless it says otherwise.
pub const BRANCH: &str = "bolt/x";

/// A bolt's gate, with the walkthrough's keys. A rig over real profiles
/// names its own runner in place of `fake`.
pub fn policy(batch: bool, flaky: &str, parallel: usize) -> String {
    format!(
        "[tests]\nrunner = \"fake\"\n\n[checkout]\n\n\
         [branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nparallel = {parallel}\n\
         batch = {batch}\nflaky = \"{flaky}\"\ntimeout = \"20m\"\n\
         trust = [\"open\", \"sandbox\", \"container\"]\n"
    )
}

/// `policy` with two ports and a preview table whose command is
/// `command` (previews-1 clauses 98, 99).
pub fn previewed(policy: &str, command: &str) -> String {
    policy.replace(
        "[checkout]\n",
        "[checkout]\nports = [\"web\", \"api\"]\n",
    ) + &format!(
        "\n[preview]\ncommand = \"{command}\"\n\
         addresses = {{ web = \"https://{{name.web}}.localhost\", api = \"http://localhost:{{port.api}}/docs\" }}\n"
    )
}

/// What a scenario needs of a world: `bolt/x` holding three projects,
/// `a/`, `b/` and `c/`, each with one check `<project>:test` tagged
/// `gate` that the project's files affect, under a policy.
pub trait Rig {
    /// A sluice over the rig's seams. Making a new one is starting sluice
    /// again: it holds nothing between calls but what the records hold.
    fn sluice(&self) -> Sluice<'_>;

    /// A session `agent` in place `worktree`, with work on the branch's
    /// current revision; returns the session's head.
    fn work(&self, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) -> Rev;

    /// What a submission from the place `worktree` names it by.
    fn place(&self, worktree: &str) -> String;

    /// The session in place `worktree` starts its work again on `on`;
    /// returns its new head.
    fn work_on(&self, worktree: &str, on: &Rev, changes: &[(&str, Option<&str>)]) -> Rev;

    /// The branch `name` at `at`, as a bolt's is kept: read without a
    /// place of it.
    fn branch_at(&self, name: &str, at: &Rev);

    /// The place `worktree` holds `paths` beyond its commits.
    fn dirty(&self, worktree: &str, paths: &[&str]);

    /// The session's worktree `worktree` is removed, with nothing released
    /// first, as a person or a driver that knows nothing of sluice removes
    /// it.
    fn remove_worktree(&self, worktree: &str);

    /// `task` fails on any candidate whose `path` holds `content`.
    fn fails_with(&self, task: &str, path: &str, content: &str);

    /// `task` runs past any time limit on every candidate: a run of it
    /// goes on until it is ended. On the fakes every run started from now
    /// goes on so.
    fn hangs(&self, task: &str);

    /// 123: every release of a combination's checkout from now fails, as
    /// a release command that exits 2 printing [`shared::RELEASE_FAILS`]
    /// does, or succeeds again.
    fn release_fails(&self, fails: bool);

    /// How many times the release of the place `name`, a combination's, a
    /// session's or a preview's, was asked of the place seam: a release
    /// command run, or refused.
    fn releases_of(&self, name: &str) -> usize;

    /// 123: how many times the release command the runs of the place
    /// `name` stored was run because its working copy was gone.
    fn gone_releases_of(&self, name: &str) -> usize;

    /// A policy written for the fakes, as this rig's own: its runner in
    /// place of `fake`.
    fn policy_text(&self, policy: &str) -> String {
        policy.into()
    }

    /// The host the rig's checks run on, as a run records it.
    fn host(&self) -> String;

    /// Step until the branch's work is done.
    fn settle(&self);

    /// Every outcome delivered to `agent`'s sessions, in order.
    fn delivered_to(&self, agent: &str) -> Vec<Notification>;

    /// A failed task's output, read where the records seam said it is kept.
    fn kept_log(&self, at: &str) -> Option<String>;

    /// The revisions runs were started on, in order.
    fn started(&self) -> Vec<Rev>;

    /// Every run the core asked the check seam for, in order.
    fn requests(&self) -> Vec<RunRequest>;

    /// The number of times the branch was moved.
    fn moves(&self) -> usize;

    /// Whether the branch is kept on a remote (C.7).
    fn on_remote(&self) -> bool;

    /// Another host's commit applying `changes` on the branch where it is
    /// kept, its remote or the host's own refs, with nothing fetched to
    /// this host: the branch's new revision (129).
    fn elsewhere(&self, changes: &[(&str, Option<&str>)]) -> Rev;

    /// The places made, and those not yet released.
    fn made_places(&self) -> Vec<String>;
    fn live_places(&self) -> Vec<String>;

    /// A commit's parents, its message, and its tree as a value equal
    /// exactly when the trees are.
    fn parents(&self, rev: &Rev) -> Vec<Rev>;
    fn message(&self, rev: &Rev) -> String;
    fn tree(&self, rev: &Rev) -> String;

    /// The branch's first-parent history, newest first.
    fn first_parents(&self) -> Vec<Rev> {
        let mut out = Vec::new();
        let mut at = Some(self.tip());
        while let Some(r) = at {
            at = self.parents(&r).first().cloned();
            out.push(r);
        }
        out
    }

    /// A file of a revision's tree.
    fn file(&self, rev: &Rev, path: &str) -> Option<String> {
        let s = self.sluice();
        s.seams()
            .branch
            .read_file(rev, path)
            .expect("the file reads")
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn tip(&self) -> Rev {
        let s = self.sluice();
        s.seams()
            .branch
            .read(BRANCH)
            .expect("the branch reads")
            .expect("the branch exists")
    }

    fn try_submit(
        &self,
        worktree: &str,
        after: &[u64],
        priority: i64,
    ) -> Result<Accepted, SubmitRefusal> {
        self.sluice()
            .submit(&SubmitRequest {
                head: SubmitHead::Worktree(self.place(worktree)),
                branch: BRANCH.into(),
                after: after.to_vec(),
                priority,
                subject: None,
            })
            .expect("submit runs")
    }

    fn submit(&self, worktree: &str) -> u64 {
        self.try_submit(worktree, &[], 0).expect("accepted").id
    }

    /// 124: the branch `branch` submitted by name, from no worktree.
    fn try_submit_by_name(&self, branch: &str) -> Result<Accepted, SubmitRefusal> {
        self.sluice()
            .submit(&SubmitRequest {
                head: SubmitHead::Branch(branch.into()),
                branch: BRANCH.into(),
                after: Vec::new(),
                priority: 0,
                subject: None,
            })
            .expect("submit runs")
    }

    fn submit_by_name(&self, branch: &str) -> u64 {
        self.try_submit_by_name(branch).expect("accepted").id
    }

    /// 125: the places' heads submitted as one act, in the order given.
    fn try_submit_group(
        &self,
        worktrees: &[&str],
        after: &[u64],
        priority: i64,
    ) -> Result<Vec<Accepted>, Vec<(String, SubmitRefusal)>> {
        self.sluice()
            .submit_group(&GroupRequest {
                branch: BRANCH.into(),
                heads: worktrees
                    .iter()
                    .map(|w| SubmitHead::Worktree(self.place(w)))
                    .collect(),
                after: after.to_vec(),
                priority,
                subjects: Vec::new(),
            })
            .expect("submit runs")
    }

    fn submit_group(&self, worktrees: &[&str]) -> Vec<u64> {
        let accepted = self.try_submit_group(worktrees, &[], 0).expect("accepted");
        accepted.iter().map(|a| a.id).collect()
    }

    fn step(&self) -> Step {
        self.sluice().step(BRANCH).expect("a step runs")
    }

    /// Session-runs-1 clause 89: a session's run of its checks in its own
    /// place, against the branch, to its end, as the core read its report,
    /// and what it recorded. The caller stands for the run `sluice exec`
    /// would count against the bound.
    fn session_run(&self, worktree: &str) -> (RunReport, Recorded) {
        self.session_exec(worktree, None)
    }

    /// A session run of the checks, or of `command`, as `session_run`.
    fn session_exec(&self, worktree: &str, command: Option<&str>) -> (RunReport, Recorded) {
        let s = self.sluice();
        let place = self.place(worktree);
        let plan = s
            .session_plan(BRANCH)
            .expect("the plan reads")
            .expect("the branch is managed");
        let session = s.seams().driver.session(&place).expect("a session's place");
        let part = shared::session_part(&s);
        let started = s
            .start_session(&plan, &place, &session.head, command, part)
            .expect("the session run starts");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(300);
        let report = loop {
            match s.seams().tests.progress(&started.run).expect("progress") {
                Progress::Running => {
                    assert!(
                        std::time::Instant::now() < until,
                        "the session run never ended"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Progress::Finished(r) => break r,
                Progress::Lost => panic!("the session run was lost"),
            }
        };
        let finished = s
            .finish_session(&plan, &session, &started, command.is_some(), &report)
            .expect("the session run records");
        assert_eq!(finished.unkept, None, "its result is kept");
        (finished.report, finished.recorded)
    }

    fn sub(&self, id: u64) -> Submission {
        self.sluice()
            .state(id)
            .expect("state reads")
            .unwrap_or_else(|| panic!("no submission {id}"))
    }

    /// The branch's records' version.
    fn version(&self) -> u64 {
        let s = self.sluice();
        s.seams().state.read(BRANCH).expect("records read").version
    }

    /// The branch's record files, by path.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        let s = self.sluice();
        s.seams().state.read(BRANCH).expect("records read").files
    }

    /// The repository's results log, as the branch's gate reads it.
    fn results(&self) -> Vec<ResultLine> {
        let s = self.sluice();
        let bytes = s.seams().state.results(BRANCH).expect("results read");
        parse_lines(&bytes).expect("the results log parses")
    }

    /// 51: the branch's deliveries, weir's records, in the order added
    /// (record 0102).
    fn deliveries(&self) -> Vec<weir_delivery::Record> {
        let s = self.sluice();
        let snap = s.seams().state.read(BRANCH).expect("records read");
        Line::load(BRANCH, snap)
            .expect("the records parse")
            .deliveries()
            .to_vec()
    }

    /// 51: the hand-overs of the outcome under `key`, by their ways, in the
    /// order recorded.
    fn handed(&self, key: &str) -> Vec<String> {
        self.deliveries()
            .into_iter()
            .filter_map(|r| match r {
                weir_delivery::Record::Handover(h) if h.key.as_str() == key => {
                    Some(h.by.to_string())
                }
                _ => None,
            })
            .collect()
    }

    /// 51: the readers that have confirmed the outcome under `key`.
    fn confirmed(&self, key: &str) -> Vec<String> {
        self.deliveries()
            .into_iter()
            .filter_map(|r| match r {
                weir_delivery::Record::Confirmation(c) if c.key.as_str() == key => {
                    Some(c.reader.to_string())
                }
                _ => None,
            })
            .collect()
    }

    /// The branch's own document, `branch.json`.
    fn doc(&self) -> BranchDoc {
        let s = self.sluice();
        let snap = s.seams().state.read(BRANCH).expect("records read");
        Line::load(BRANCH, snap).expect("the records parse").doc
    }

    fn log(&self) -> Vec<LogLine> {
        let s = self.sluice();
        let snap = s.seams().state.read(BRANCH).expect("records read");
        Line::read_log(&snap).expect("the log parses")
    }

    fn events(&self) -> Vec<Event> {
        self.log().into_iter().map(|l| l.event).collect()
    }

    /// The combinations runs were started on, as lists of submissions.
    fn tested(&self) -> Vec<Vec<u64>> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::Testing { submissions, .. } => Some(submissions),
                _ => None,
            })
            .collect()
    }

    fn withdraw(&self, id: u64, by: Requester) {
        self.sluice()
            .withdraw(id, &by)
            .expect("withdraw runs")
            .expect("withdrawn");
    }
}

/// The rig over the fakes.
pub struct Harness {
    pub f: Fakes,
}

impl Harness {
    /// The three projects under the given policy.
    pub fn new(policy: &str) -> Harness {
        let f = Fakes::new();
        f.world.init(
            BRANCH,
            &[
                (".config/sluice.toml", policy),
                ("a/lib", "a1"),
                ("b/lib", "b1"),
                ("c/lib", "c1"),
            ],
        );
        for p in ["a", "b", "c"] {
            f.world.task(&format!("{p}:test"), &format!("{p}/"));
        }
        Harness { f }
    }

    /// Every candidate tested, a flaky key read as failed, a window of
    /// four.
    pub fn bolt() -> Harness {
        Harness::new(&policy(false, "fail", 4))
    }

    pub fn w(&self) -> &World {
        &self.f.world
    }
}

impl Rig for Harness {
    fn sluice(&self) -> Sluice<'_> {
        self.f.sluice()
    }

    fn kept_log(&self, at: &str) -> Option<String> {
        self.w().kept_log(at)
    }

    fn work(&self, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) -> Rev {
        let head = self.w().work(&self.tip(), changes);
        self.w().session(worktree, agent, &head);
        head
    }

    fn place(&self, worktree: &str) -> String {
        worktree.into()
    }

    fn work_on(&self, worktree: &str, on: &Rev, changes: &[(&str, Option<&str>)]) -> Rev {
        let session = self.f.driver.session(worktree).expect("a session's place");
        let head = self.w().work(on, changes);
        self.w().session(worktree, &session.from.agent, &head);
        head
    }

    fn branch_at(&self, name: &str, at: &Rev) {
        self.w().set_branch(name, at);
    }

    fn dirty(&self, worktree: &str, paths: &[&str]) {
        self.w().dirty(worktree, paths);
    }

    fn remove_worktree(&self, worktree: &str) {
        self.w().remove_worktree(worktree);
    }

    fn fails_with(&self, task: &str, path: &str, content: &str) {
        self.w().fails_with(task, path, content);
    }

    fn hangs(&self, task: &str) {
        self.w().hangs(task);
        self.w().hold_runs(true);
    }

    fn release_fails(&self, fails: bool) {
        self.w()
            .release_fails(fails.then_some(shared::RELEASE_FAILS));
    }

    fn releases_of(&self, name: &str) -> usize {
        self.w()
            .release_attempts()
            .iter()
            .filter(|p| *p == name)
            .count()
    }

    fn gone_releases_of(&self, name: &str) -> usize {
        self.w()
            .gone_releases()
            .iter()
            .filter(|p| *p == name)
            .count()
    }

    fn host(&self) -> String {
        "fake-host".into()
    }

    /// Step until a step changes nothing.
    fn settle(&self) {
        for _ in 0..64 {
            let v = self.w().version(BRANCH);
            let delivered = self.w().delivered().len();
            let places = self.w().made_places().len();
            self.step();
            if v == self.w().version(BRANCH)
                && delivered == self.w().delivered().len()
                && places == self.w().made_places().len()
            {
                return;
            }
        }
        panic!("{BRANCH} never settled");
    }

    fn delivered_to(&self, agent: &str) -> Vec<Notification> {
        self.w().delivered_to(agent)
    }

    fn started(&self) -> Vec<Rev> {
        self.w().started()
    }

    fn requests(&self) -> Vec<RunRequest> {
        self.w().requests()
    }

    fn on_remote(&self) -> bool {
        self.w().is_on_remote(BRANCH)
    }

    fn elsewhere(&self, changes: &[(&str, Option<&str>)]) -> Rev {
        self.w().hand_commit(BRANCH, changes)
    }

    fn moves(&self) -> usize {
        self.w().moves().len()
    }

    fn made_places(&self) -> Vec<String> {
        self.w().made_places()
    }

    fn live_places(&self) -> Vec<String> {
        self.w().live_places()
    }

    fn parents(&self, rev: &Rev) -> Vec<Rev> {
        self.w().parents(rev)
    }

    fn message(&self, rev: &Rev) -> String {
        self.w().message(rev)
    }

    fn tree(&self, rev: &Rev) -> String {
        format!("{:?}", self.w().tree(rev))
    }

    fn first_parents(&self) -> Vec<Rev> {
        self.w().first_parents(BRANCH)
    }
}
