//! The check seam's `moon` runner on a scratch moon workspace: section 3's
//! acceptance tests in `docs/plans/phase-1-integrations.md`. Needs moon
//! 2.6 or later on the path (sluice's devenv), so it is in the
//! check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sluice::instance::{HostCache, Instance};
use sluice::profiles::checks::Checks;
use sluice_seams::{
    FailedBy, Limits, Network, NotRun, Outcome, Place, Progress, Reading, Reads, Refused, Rev,
    RunHandle, RunReport, RunRequest, Setting, Settings, Source, TaskState, Tests,
};
use support::Repo;

/// Two projects, `beta:test` depending on `alpha:test`, both tagged
/// `check-fast`; `gamma:test` tagged `check-tools`.
fn workspace(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write(
        ".moon/workspace.yml",
        "projects:\n  alpha: 'alpha'\n  beta: 'beta'\n  gamma: 'gamma'\n\
         vcs:\n  client: 'git'\n  defaultBranch: 'main'\ntelemetry: false\n",
    );
    repo.write(
        "alpha/moon.yml",
        "tasks:\n  test:\n    command: 'sh test.sh'\n    inputs: ['test.sh', 'value.txt', 'notes.md']\n    tags: [check-fast]\n",
    );
    repo.write(
        "alpha/test.sh",
        "grep -qx ok value.txt || { echo \"alpha: $(cat value.txt)\"; exit 1; }\n",
    );
    repo.write(
        "beta/moon.yml",
        "dependsOn: ['alpha']\ntasks:\n  test:\n    command: 'sh test.sh'\n    inputs: ['test.sh', 'value.txt', 'notes.md', '/alpha/value.txt']\n    deps: ['alpha:test']\n    tags: [check-fast]\n",
    );
    repo.write("beta/test.sh", "grep -qx ok value.txt\n");
    repo.write(
        "gamma/moon.yml",
        "tasks:\n  test:\n    command: 'true'\n    inputs: ['value.txt']\n    tags: [check-tools]\n",
    );
    for p in ["alpha", "beta", "gamma"] {
        repo.write(&format!("{p}/value.txt"), "ok\n");
        repo.write(&format!("{p}/notes.md"), "notes\n");
    }
    repo.write(".gitignore", ".moon/cache/\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "workspace"]);
    repo
}

/// A commit on HEAD changing `changes`; the place moves to it.
fn change(repo: &Repo, changes: &[(&str, &str)]) -> String {
    for (p, c) in changes {
        repo.write(p, c);
    }
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "change"]);
    repo.rev("HEAD")
}

fn checks(repo: &Repo) -> Checks {
    caching(repo, Vec::new())
}

/// The check profile of `repo` on a host that declares `caches`.
fn caching(repo: &Repo, caches: Vec<HostCache>) -> Checks {
    checks_in(&repo.dir().join(".git"), caches)
}

/// The check profile of the repository whose common directory is
/// `common`, on a host that declares `caches`.
fn checks_in(common: &Path, caches: Vec<HostCache>) -> Checks {
    Checks::new(
        common,
        PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
        Instance {
            bound: 2,
            reserved: 0,
            processors: 8,
            forge: None,
            signing: None,
            peers: Default::default(),
            systems: None,
            caches,
            expire_after: sluice_core::EXPIRE_AFTER,
            telemetry: None,
        },
    )
    .with_running(common.join("host-running"))
}

fn moon() -> Settings {
    Settings::from([("runner".into(), Setting::Text("moon".into()))])
}

fn request(repo: &Repo, name: &str, required: &[&str], since: Option<&str>) -> RunRequest {
    let tmp = repo.dir().join(".git/sluice/.tmp").join(name);
    std::fs::create_dir_all(&tmp).unwrap();
    RunRequest {
        place: Place {
            name: name.into(),
            path: repo.dir().to_path_buf(),
            tmp,
            env: Default::default(),
        },
        revision: Rev::new(repo.rev("HEAD")),
        required: required.iter().map(|s| s.to_string()).collect(),
        since: since.map(Rev::new),
        limit: Some(Duration::from_secs(120)),
        limits: Limits::default(),
        tests: moon(),
        command: None,
        replay: true,
        system: None,
    }
}

fn finish(c: &Checks, req: &RunRequest) -> RunReport {
    finish_run(c, req).1
}

fn finish_run(c: &Checks, req: &RunRequest) -> (RunHandle, RunReport) {
    let run = support::start(c, req).unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        match c.progress(&run).unwrap() {
            Progress::Finished(r) => return (run, r),
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn tasks(r: &RunReport) -> Vec<(String, Outcome, bool)> {
    r.tasks
        .iter()
        .map(|t| {
            (
                t.task.clone(),
                t.outcome,
                matches!(t.source, Source::Replayed { .. }),
            )
        })
        .collect()
}

fn names(r: &RunReport) -> Vec<String> {
    r.tasks.iter().map(|t| t.task.clone()).collect()
}

/// 36, 70: the tasks run are the required ones the range affects,
/// dependents included; moon's own configuration affects every one.
#[test]
fn the_tasks_a_range_affects() {
    let repo = workspace("moon-affected");
    let c = checks(&repo);
    let fast = ["#check-fast"];

    let base = repo.rev("HEAD");
    change(
        &repo,
        &[("alpha/value.txt", "ok\n"), ("alpha/notes.md", "more\n")],
    );
    let r = finish(&c, &request(&repo, "p1", &fast, Some(&base)));
    assert_eq!(names(&r), ["alpha:test", "beta:test"]);

    let base = repo.rev("HEAD");
    change(&repo, &[("beta/notes.md", "more\n")]);
    let r = finish(&c, &request(&repo, "p2", &fast, Some(&base)));
    assert_eq!(names(&r), ["beta:test"]);

    let base = repo.rev("HEAD");
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    change(
        &repo,
        &[(".moon/workspace.yml", &format!("{ws}# changed\n"))],
    );
    let r = finish(&c, &request(&repo, "p3", &fast, Some(&base)));
    assert_eq!(names(&r), ["alpha:test", "beta:test"]);
}

/// 71, 83; the run-bound spec's "Six cores each": the moon runner hands
/// moon the run's share of the cores as `--concurrency`, the
/// `SLUICE_JOBS` its environment carries.
#[test]
fn moon_is_given_the_runs_share_of_the_cores() {
    let repo = workspace("moon-share");
    let c = checks(&repo);
    let base = repo.rev("HEAD");
    change(&repo, &[("alpha/notes.md", "shared\n")]);
    finish(&c, &request(&repo, "p1", &["#check-fast"], Some(&base)));
    let req = c.runs().request("p1").unwrap();
    let jobs = &req.env["SLUICE_JOBS"];
    assert_eq!(jobs, "4", "eight cores under a bound of two");
    let at = req
        .argv
        .iter()
        .position(|a| a == "--concurrency")
        .unwrap_or_else(|| panic!("no --concurrency in {:?}", req.argv));
    assert_eq!(req.argv.get(at + 1), Some(jobs), "{:?}", req.argv);
}

/// ADR 0028: while a run goes, its tasks read as moon writes them: one
/// ended passes with its times, one started and not ended runs, and once
/// the run has finished there is nothing to say, its report being the
/// result (37, 38).
#[test]
fn a_run_shows_each_task_as_it_ends() {
    shows_each_task_as_it_ends("moon-progress", |_| "p1".into());
}

/// ADR 0035: a session's run, which takes a new run name in its place each
/// time (session-runs-1 clause 94), reads its tasks as a combination's do.
#[test]
fn a_session_run_shows_each_task_as_it_ends() {
    shows_each_task_as_it_ends("moon-session-progress", |repo| {
        format!("session/{}", repo.dir().display())
    });
}

fn shows_each_task_as_it_ends(name: &str, place: impl Fn(&Repo) -> String) {
    let repo = workspace(name);
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    change(
        &repo,
        &[
            (
                ".moon/workspace.yml",
                &ws.replace("  gamma: 'gamma'\n", "  gamma: 'gamma'\n  slow: 'slow'\n"),
            ),
            (
                "slow/moon.yml",
                "tasks:\n  test:\n    command: 'sh wait.sh'\n    inputs: ['wait.sh']\n    tags: [check-fast]\n",
            ),
            (
                "slow/wait.sh",
                "while [ ! -f ../flag ]; do sleep 0.1; done\n",
            ),
            (".gitignore", ".moon/cache/\nflag\n"),
        ],
    );
    let c = checks(&repo);
    let req = request(&repo, &place(&repo), &["alpha:test", "slow:test"], None);
    let run = support::start(&c, &req).unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    let seen = loop {
        let tasks = c.tasks(&run).unwrap();
        let task = |t: &str| tasks.iter().find(|x| x.target == t);
        // moon writes a task's run record a moment before its end
        // checkpoint, which gives its start.
        // A running task's start is when its start line was printed,
        // noted by the supervisor within a second (ADR 0042).
        if task("alpha:test").is_some_and(|t| t.state == TaskState::Passed && t.started.is_some())
            && task("slow:test")
                .is_some_and(|t| t.state == TaskState::Running && t.started.is_some())
        {
            break tasks;
        }
        assert!(
            Instant::now() < until,
            "alpha:test never passed with slow:test running since a time: {tasks:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let alpha = seen.iter().find(|t| t.target == "alpha:test").unwrap();
    assert_eq!(alpha.exit, Some(0));
    assert!(alpha.ended.is_some());
    assert!(alpha.started <= alpha.ended);
    assert!(matches!(c.progress(&run).unwrap(), Progress::Running));

    std::fs::write(repo.dir().join("flag"), "").unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    let report = loop {
        match c.progress(&run).unwrap() {
            Progress::Finished(r) => break r,
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        tasks(&report),
        [
            ("alpha:test".into(), Outcome::Pass, false),
            ("slow:test".into(), Outcome::Pass, false)
        ]
    );
    assert!(c.tasks(&run).unwrap().is_empty());
}

/// ADR 0028, 20: a project whose id holds a `/` keeps its tasks' run
/// records and logs where moon names them, `group-many/<task>`; each
/// ended task reads with its times and exit, and a failed one's log is
/// its own, not the run's.
#[test]
fn a_project_whose_id_holds_a_slash() {
    let repo = workspace("moon-slash");
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    change(
        &repo,
        &[
            (
                ".moon/workspace.yml",
                &ws.replace(
                    "  gamma: 'gamma'\n",
                    "  gamma: 'gamma'\n  group/many: 'group/many'\n",
                ),
            ),
            (
                "group/many/moon.yml",
                "tasks:\n  pass:\n    command: 'true'\n  fail:\n    command: 'sh fail.sh'\n  wait:\n    command: 'sh wait.sh'\n",
            ),
            ("group/many/fail.sh", "echo 'fail: broken'\nexit 3\n"),
            (
                "group/many/wait.sh",
                "while [ ! -f ../../flag ]; do sleep 0.1; done\n",
            ),
            (".gitignore", ".moon/cache/\nflag\n"),
        ],
    );
    let c = checks(&repo);
    let targets = ["group/many:fail", "group/many:pass", "group/many:wait"];
    let req = request(&repo, "p1", &targets, None);
    let run = support::start(&c, &req).unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    let seen = loop {
        let tasks = c.tasks(&run).unwrap();
        let task = |t: &str| tasks.iter().find(|x| x.target == t);
        let ended = |t: &str, state| {
            task(t).is_some_and(|t| t.state == state && t.started.is_some() && t.ended.is_some())
        };
        if ended("group/many:pass", TaskState::Passed)
            && ended("group/many:fail", TaskState::Failed)
            && task("group/many:wait").map(|t| t.state) == Some(TaskState::Running)
        {
            break tasks;
        }
        assert!(
            Instant::now() < until,
            "group/many's tasks never ended with group/many:wait running: {tasks:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let exit = |t: &str| seen.iter().find(|x| x.target == t).unwrap().exit;
    assert_eq!(exit("group/many:pass"), Some(0));
    assert_eq!(exit("group/many:fail"), Some(3));

    std::fs::write(repo.dir().join("flag"), "").unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    let report = loop {
        match c.progress(&run).unwrap() {
            Progress::Finished(r) => break r,
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        tasks(&report),
        [
            ("group/many:fail".into(), Outcome::Fail, false),
            ("group/many:pass".into(), Outcome::Pass, false),
            ("group/many:wait".into(), Outcome::Pass, false)
        ]
    );
    // The run's own output holds the line too, prefixed with the target;
    // only the task's own log starts with its stream.
    let logs = c.logs(&run, &["group/many:fail".into()]).unwrap();
    assert!(
        logs[0].text.starts_with("--- stdout ---\n") && logs[0].text.contains("fail: broken"),
        "{}",
        logs[0].text
    );
}

/// Time-limit-1 66: a run its time limit ends keeps the outcomes of the
/// tasks that ended before it, a pass and a fail; a task it cut off has
/// none, whether it died of the signal or caught it and exited as
/// `bun test` does, so no fail is recorded under its key, and its log is
/// its own. The project's id holds a `/`, as `scripts/ci`'s does.
#[test]
fn a_task_the_time_limit_cut_off_has_no_outcome() {
    let repo = workspace("moon-cut-off");
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    change(
        &repo,
        &[
            (
                ".moon/workspace.yml",
                &ws.replace(
                    "  gamma: 'gamma'\n",
                    "  gamma: 'gamma'\n  group/many: 'group/many'\n",
                ),
            ),
            (
                "group/many/moon.yml",
                "tasks:\n  pass:\n    command: 'true'\n  fail:\n    command: 'sh fail.sh'\n  hang:\n    command: 'sh hang.sh'\n  catch:\n    command: 'sh catch.sh'\n",
            ),
            ("group/many/fail.sh", "echo 'fail: broken'\nexit 3\n"),
            (
                "group/many/hang.sh",
                "echo 'hang: still going'\nwhile :; do sleep 0.1; done\n",
            ),
            (
                "group/many/catch.sh",
                "trap 'exit 143' TERM\necho 'catch: still going'\nwhile :; do sleep 0.1; done\n",
            ),
        ],
    );
    let c = checks(&repo);
    let targets = [
        "group/many:catch",
        "group/many:fail",
        "group/many:hang",
        "group/many:pass",
    ];
    let mut req = request(&repo, "p1", &targets, None);
    let limit = Duration::from_secs(15);
    req.limit = Some(limit);
    let started = Instant::now();
    let run = support::start(&c, &req).unwrap();
    // The two that end do so well inside the limit, or the test says
    // nothing of a task that ended before it.
    loop {
        let tasks = c.tasks(&run).unwrap();
        let state = |t: &str| tasks.iter().find(|x| x.target == t).map(|x| x.state);
        if state("group/many:pass") == Some(TaskState::Passed)
            && state("group/many:fail") == Some(TaskState::Failed)
            && state("group/many:hang") == Some(TaskState::Running)
            && state("group/many:catch") == Some(TaskState::Running)
        {
            break;
        }
        assert!(
            started.elapsed() < limit - Duration::from_secs(3),
            "group/many's tasks never ended with group/many:hang running: {tasks:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let until = Instant::now() + Duration::from_secs(120);
    let report = loop {
        match c.progress(&run).unwrap() {
            Progress::Finished(r) => break r,
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(report.timed_out);
    assert_eq!(
        tasks(&report),
        [
            ("group/many:fail".into(), Outcome::Fail, false),
            ("group/many:pass".into(), Outcome::Pass, false)
        ]
    );
    assert_eq!(report.missing, ["group/many:catch", "group/many:hang"]);
    let logs = c
        .logs(&run, &["group/many:catch".into(), "group/many:hang".into()])
        .unwrap();
    for (log, line) in logs.iter().zip(["catch: still going", "hang: still going"]) {
        assert!(
            log.text.contains(line) && !log.text.contains("fail: broken"),
            "{}",
            log.text
        );
    }
}

/// 15, 37: a candidate that affects no required task runs nothing and
/// passes.
#[test]
fn an_unaffected_candidate_runs_nothing() {
    let repo = workspace("moon-unaffected");
    let c = checks(&repo);
    let base = repo.rev("HEAD");
    change(&repo, &[("README", "more\n")]);
    let r = finish(&c, &request(&repo, "p1", &["#check-fast"], Some(&base)));
    assert!(r.tasks.is_empty() && r.missing.is_empty() && !r.timed_out);
    assert!(!repo.dir().join(".moon/cache/runReport.json").exists());
}

/// 16, 38, rule core.9: a first run observes each task with its key; a
/// second in the same place replays them under the same keys.
#[test]
fn a_second_run_replays_under_the_same_keys() {
    let repo = workspace("moon-replay");
    let c = checks(&repo);
    let first = finish(&c, &request(&repo, "p1", &["#check-fast"], None));
    assert_eq!(
        tasks(&first),
        [
            ("alpha:test".into(), Outcome::Pass, false),
            ("beta:test".into(), Outcome::Pass, false)
        ]
    );
    let second = finish(&c, &request(&repo, "p2", &["#check-fast"], None));
    assert_eq!(
        tasks(&second),
        [
            ("alpha:test".into(), Outcome::Pass, true),
            ("beta:test".into(), Outcome::Pass, true)
        ]
    );
    let keys = |r: &RunReport| r.tasks.iter().map(|t| t.hash.clone()).collect::<Vec<_>>();
    assert_eq!(keys(&first), keys(&second));
    assert!(keys(&first).iter().all(|h| h.len() == 64));
    assert_eq!(first.executor.kind, "open");
    assert_eq!(first.executor.processors, Some(4));
}

/// 28: a request that may not replay runs every task again, under the
/// same keys, though each key holds a pass.
#[test]
fn a_run_without_replay_runs_every_task() {
    let repo = workspace("moon-no-replay");
    let c = checks(&repo);
    let first = finish(&c, &request(&repo, "p1", &["#check-fast"], None));
    let mut req = request(&repo, "p2", &["#check-fast"], None);
    req.replay = false;
    let second = finish(&c, &req);
    assert_eq!(
        tasks(&second),
        [
            ("alpha:test".into(), Outcome::Pass, false),
            ("beta:test".into(), Outcome::Pass, false)
        ]
    );
    let keys = |r: &RunReport| r.tasks.iter().map(|t| t.hash.clone()).collect::<Vec<_>>();
    assert_eq!(keys(&first), keys(&second));
}

/// 15, 16, 149: a failing task is a fail under its key; its dependent,
/// which moon skipped for it, is failed by it, and never missing.
#[test]
fn a_failure_and_its_dependent() {
    let repo = workspace("moon-fail");
    let c = checks(&repo);
    change(&repo, &[("alpha/value.txt", "broken\n")]);
    let (run, r) = finish_run(&c, &request(&repo, "p1", &["#check-fast"], None));
    assert_eq!(tasks(&r), [("alpha:test".into(), Outcome::Fail, false)]);
    assert_eq!(r.tasks[0].hash.len(), 64);
    assert_eq!(
        r.failed_by,
        [FailedBy {
            task: "beta:test".into(),
            by: vec!["alpha:test".into()],
        }]
    );
    // A dependency that is itself required is listed as a check, once.
    assert!(r.dependencies.is_empty(), "{:?}", r.dependencies);
    assert!(r.missing.is_empty(), "{:?}", r.missing);
    // 20: what the failed task printed, as moon kept it; the dependent
    // moon never ran has the run's own output.
    let logs = c
        .logs(&run, &["alpha:test".into(), "beta:test".into()])
        .unwrap();
    assert!(logs[0].text.contains("alpha: broken"), "{}", logs[0].text);
    assert!(!logs[1].text.is_empty());
}

/// could-not-run-1 114, 149: a task that exits as the policy's `not_run`
/// says could not run here, so no fail is recorded under its key; its
/// dependent, which moon skipped for it, is not run, saying so.
#[test]
fn a_task_that_could_not_run_records_no_fail() {
    let repo = workspace("moon-not-run");
    let c = checks(&repo);
    change(&repo, &[("alpha/test.sh", "exit 77\n")]);
    let mut req = request(&repo, "p1", &["#check-fast"], None);
    req.tests
        .insert("not_run".into(), Setting::List(vec!["77".into()]));
    let r = finish(&c, &req);
    assert!(
        !r.tasks
            .iter()
            .any(|t| t.task == "alpha:test" && t.outcome == Outcome::Fail),
        "recorded as failed: {:?}",
        r.tasks
    );
    assert_eq!(r.not_run.len(), 2, "{:?}", r.not_run);
    assert_eq!(r.not_run[0].task, "alpha:test");
    assert!(r.not_run[0].reason.contains("exited 77"));
    assert_eq!(r.not_run[1].task, "beta:test");
    assert!(
        r.not_run[1]
            .reason
            .starts_with("its dependency alpha:test could not run: exited 77"),
        "{}",
        r.not_run[1].reason
    );
    assert!(r.failed_by.is_empty() && r.dependencies.is_empty());
    assert!(r.missing.is_empty(), "{:?}", r.missing);
}

/// The workspace with a `root` project whose `all` task runs nothing of
/// its own, `noop`, and depends on what `deps` names, in moon.yml's words.
fn noop_workspace(name: &str, deps: &str) -> Repo {
    let repo = workspace(name);
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    repo.write(
        ".moon/workspace.yml",
        &ws.replace("projects:\n", "projects:\n  root: 'root'\n"),
    );
    repo.write(
        "root/moon.yml",
        &format!("tasks:\n  all:\n    command: 'noop'\n    deps:\n{deps}"),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "root"]);
    repo
}

/// 149, 16, 20; the failed-dependency spec's "The run report from moon":
/// a required `root:all` that runs nothing and depends on `alpha:test`,
/// which no gate requires, is failed by it when it fails; `alpha:test`'s
/// result is the run's, ran here under its own key, and its log is what
/// it printed, as moon kept it.
#[test]
fn a_noop_check_is_failed_by_the_task_it_depends_on() {
    let repo = noop_workspace("moon-noop", "      - 'alpha:test'\n");
    let c = checks(&repo);
    change(&repo, &[("alpha/value.txt", "broken\n")]);
    let (run, r) = finish_run(&c, &request(&repo, "p1", &["root:all"], None));
    assert!(r.tasks.is_empty(), "{:?}", r.tasks);
    assert!(r.missing.is_empty(), "{:?}", r.missing);
    assert!(r.not_run.is_empty(), "{:?}", r.not_run);
    assert_eq!(
        r.failed_by,
        [FailedBy {
            task: "root:all".into(),
            by: vec!["alpha:test".into()],
        }]
    );
    assert_eq!(r.dependencies.len(), 1, "{:?}", r.dependencies);
    let dep = &r.dependencies[0];
    assert_eq!(
        (dep.task.as_str(), dep.outcome),
        ("alpha:test", Outcome::Fail)
    );
    assert_eq!(dep.hash.len(), 64, "{}", dep.hash);
    assert!(matches!(dep.source, Source::Ran { .. }), "{:?}", dep.source);
    let logs = c.logs(&run, &["alpha:test".into()]).unwrap();
    assert!(logs[0].text.contains("alpha: broken"), "{}", logs[0].text);
}

/// 149: a task moon runs after a check to clean up fails no check: moon
/// runs the check, which passes, and the cleanup's fail is no
/// dependency's.
#[test]
fn a_cleanup_dependency_that_fails_fails_no_check() {
    let repo = noop_workspace(
        "moon-cleanup",
        "      - target: 'alpha:stop'\n        type: 'cleanup'\n",
    );
    let alpha = std::fs::read_to_string(repo.dir().join("alpha/moon.yml")).unwrap();
    repo.write(
        "alpha/moon.yml",
        &format!(
            "{alpha}  stop:\n    command: 'sh -c \"exit 1\"'\n    options: {{ cache: false }}\n"
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "cleanup"]);
    let c = checks(&repo);
    let r = finish(&c, &request(&repo, "p1", &["root:all"], None));
    assert_eq!(tasks(&r), [("root:all".into(), Outcome::Pass, false)]);
    assert!(r.failed_by.is_empty(), "{:?}", r.failed_by);
    assert!(r.dependencies.is_empty(), "{:?}", r.dependencies);
    assert!(r.missing.is_empty(), "{:?}", r.missing);
}

/// 70: with no verified revision every required task runs; two tags run
/// the union of their tasks, and a tag with no tasks adds none.
#[test]
fn every_required_task_of_every_tag() {
    let repo = workspace("moon-tags");
    let c = checks(&repo);
    let r = finish(
        &c,
        &request(
            &repo,
            "p1",
            &["#check-fast", "#check-tools", "#check-none"],
            None,
        ),
    );
    assert_eq!(names(&r), ["alpha:test", "beta:test", "gamma:test"]);
    let r = finish(&c, &request(&repo, "p2", &["gamma:test"], None));
    assert_eq!(names(&r), ["gamma:test"]);
}

/// ADR 0044: moon runs in its CI mode, so a required task the repository
/// keeps out of CI is not run: it reads missing, and the run names it kept
/// out. Each task's time and the run's own come back with the results.
#[test]
fn a_task_kept_out_of_ci_is_not_run() {
    let repo = workspace("moon-ci");
    repo.write(
        "gamma/moon.yml",
        "tasks:\n  test:\n    command: 'true'\n    inputs: ['value.txt']\n    tags: [check-tools]\n  \
         serve:\n    command: 'touch served'\n    tags: [check-tools]\n    options:\n      runInCI: false\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "serve"]);
    let c = checks(&repo);
    let r = finish(&c, &request(&repo, "p1", &["#check-tools"], None));
    assert_eq!(names(&r), ["gamma:test"]);
    assert_eq!(r.missing, ["gamma:serve"]);
    assert_eq!(r.kept_out, ["gamma:serve"]);
    assert!(
        !repo.dir().join("gamma/served").exists(),
        "kept out, never run"
    );
    assert!(r.tasks[0].took_ms.is_some());
    assert_eq!(r.tasks[0].attempts, Some(1));
    assert!(r.detail.as_ref().is_some_and(|d| d.took_ms.is_some()));
}

/// 140: the affected reading, asked outside any run: a file no task
/// reads reads no check, an input reads its task and its dependents, and
/// moon's own configuration every check; the worktree it read in is gone
/// once it has answered.
#[test]
fn the_reading_names_the_checks_a_change_reads() {
    let repo = workspace("moon-reads");
    let c = checks(&repo);
    let base = Rev::new(repo.rev("HEAD"));
    let docs = Rev::new(change(&repo, &[("README.md", "docs\n")]));
    let input = Rev::new(change(&repo, &[("alpha/notes.md", "read\n")]));
    let ws = std::fs::read_to_string(repo.dir().join(".moon/workspace.yml")).unwrap();
    let config = Rev::new(change(
        &repo,
        &[(".moon/workspace.yml", &format!("{ws}# changed\n"))],
    ));
    let reads = c
        .reads(&Reading {
            required: vec!["#check-fast".into()],
            tests: moon(),
            pairs: vec![(base, docs.clone()), (docs, input.clone()), (input, config)],
        })
        .unwrap();
    assert_eq!(
        reads,
        [
            Reads::Checks(Vec::new()),
            Reads::Checks(vec!["alpha:test".into(), "beta:test".into()]),
            Reads::Every,
        ]
    );
    let runs = repo.dir().join(".git/sluice/.runs");
    let left: Vec<_> = std::fs::read_dir(&runs)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    let list = repo.git(&["worktree", "list", "--porcelain"]);
    assert_eq!(list.matches("worktree ").count(), 1, "{list}");
}

/// 140: the command runner's one check reads every change.
#[test]
fn the_command_runner_reads_every_check() {
    let repo = workspace("command-reads");
    let c = checks(&repo);
    let base = Rev::new(repo.rev("HEAD"));
    let docs = Rev::new(change(&repo, &[("README.md", "docs\n")]));
    let tests = Settings::from([
        ("runner".into(), Setting::Text("command".into())),
        ("command".into(), Setting::Text("true".into())),
    ]);
    let reads = c
        .reads(&Reading {
            required: vec!["true".into()],
            tests,
            pairs: vec![(base, docs)],
        })
        .unwrap();
    assert_eq!(reads, [Reads::Every]);
}

/// Run in a child of this test binary, with moon's options set in
/// sluice's own environment.
#[test]
fn moon_env_probe() {
    let Ok(out) = std::env::var("SLUICE_TEST_MOON_OUT") else {
        return;
    };
    let repo = workspace("moon-env");
    let c = checks(&repo);
    let first = finish(&c, &request(&repo, "p1", &["#check-fast"], None));
    let base = repo.rev("HEAD");
    change(&repo, &[("beta/notes.md", "more\n")]);
    let second = finish(&c, &request(&repo, "p2", &["#check-fast"], Some(&base)));
    std::fs::write(out, format!("{:?}\n{:?}\n", tasks(&first), tasks(&second))).unwrap();
}

/// Nothing of `MOON_*` in sluice's own environment reaches moon: a forced
/// run and a nonsense base change neither which tasks run nor whether
/// they replay.
#[test]
fn moon_options_in_sluices_environment_change_nothing() {
    let out = support::Scratch::new("moon-env-out");
    let file = out.path().join("seen");
    let status = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", "moon_env_probe", "--nocapture"])
    .env("SLUICE_TEST_MOON_OUT", &file)
    .env("MOON_FORCE", "1")
    .env("MOON_BASE", "nonsense")
    .env("MOON_AFFECTED", "1")
    .status()
    .unwrap();
    assert!(status.success());
    let seen = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        seen,
        "[(\"alpha:test\", Pass, false), (\"beta:test\", Pass, false)]\n\
         [(\"beta:test\", Pass, false)]\n"
    );
}

/// 147, C.2: a moon below 2.6 fails the version check, and the run's
/// start with it, in words naming its version and moon 2.6.
#[test]
fn an_old_moon_is_refused() {
    let repo = workspace("moon-old");
    for (version, dir) in [("1.41.7", "m1"), ("2.5.3", "m2")] {
        let bin = repo.dir().join(".git").join(dir);
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("moon"),
            format!("#!/bin/sh\necho moon {version}\n"),
        )
        .unwrap();
        // A toolchain that puts this moon first.
        let enter = bin.join("enter");
        std::fs::write(
            &enter,
            format!("#!/bin/sh\necho 'export PATH={}:$PATH'\n", bin.display()),
        )
        .unwrap();
        for f in [bin.join("moon"), enter.clone()] {
            std::fs::set_permissions(f, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        }
        let mut req = request(&repo, "p1", &["#check-fast"], None);
        req.tests.insert(
            "toolchain".into(),
            Setting::List(vec![enter.to_string_lossy().into()]),
        );
        let err = format!("{:#}", support::start(&checks(&repo), &req).unwrap_err());
        assert!(err.starts_with("moon --version: "), "{err}");
        assert!(err.contains(&format!("moon {version}")), "{err}");
        assert!(err.contains("moon 2.6"), "{err}");
    }
}

/// 31, 37: a check with a `wait` dependency on a helper and a `cleanup`
/// dependency: the helper is not a result, the check's own outcome is,
/// and a task allowed to fail that fails is a fail (27).
#[test]
fn helpers_are_not_results_and_allowed_failures_fail() {
    let repo = workspace("moon-helpers");
    repo.write(
        "alpha/moon.yml",
        "tasks:\n\
         \x20 serve:\n    command: 'sh -c \"echo up > $SLUICE_TMP/up; sleep 2\"'\n    options: { persistent: true, cache: false, runInCI: true }\n\
         \x20 stop:\n    command: 'sh -c \"echo stopped > $SLUICE_TMP/stopped\"'\n    options: { cache: false, runInCI: true }\n\
         \x20 test:\n    command: 'sh -c \"sleep 1; test -f $SLUICE_TMP/up\"'\n    inputs: ['value.txt']\n\
         \x20   deps:\n      - { target: 'alpha:serve', type: 'wait' }\n      - { target: 'alpha:stop', type: 'cleanup' }\n\
         \x20   tags: [check-fast]\n\
         \x20 lax:\n    command: 'sh -c \"exit 1\"'\n    inputs: ['value.txt']\n    options: { allowFailure: true }\n    tags: [check-fast]\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "helpers"]);
    let c = checks(&repo);
    let req = request(
        &repo,
        "p1",
        &["alpha:test", "alpha:lax", "alpha:serve"],
        None,
    );
    let r = finish(&c, &req);
    assert_eq!(
        tasks(&r),
        [
            ("alpha:lax".into(), Outcome::Fail, false),
            ("alpha:test".into(), Outcome::Pass, false)
        ]
    );
    // The helper ran only as a helper: required by name, it has no result.
    assert_eq!(r.missing, ["alpha:serve"]);
    assert!(req.place.tmp.join("stopped").exists());
}

/// 83, 84: moon's run under the sandbox, kept off the network: the tasks
/// run and replay as in the open, and the record says sandbox.
#[cfg(target_os = "macos")]
#[test]
fn a_sandboxed_run_reads_as_an_open_one() {
    let repo = workspace("moon-sandbox");
    let c = checks(&repo);
    let mut req = request(&repo, "p1", &["#check-fast"], None);
    req.limits = Limits {
        executor: Some("sandbox".into()),
        network: Some(Network::Denied),
        memory: None,
        docker: false,
    };
    let first = finish(&c, &req);
    assert_eq!(
        tasks(&first),
        [
            ("alpha:test".into(), Outcome::Pass, false),
            ("beta:test".into(), Outcome::Pass, false)
        ]
    );
    assert_eq!(first.executor.kind, "sandbox");
    assert_eq!(first.executor.network, Some(Network::Denied));
    req.place.name = "p2".into();
    let second = finish(&c, &req);
    assert!(tasks(&second).iter().all(|t| t.2), "{:?}", tasks(&second));
}

/// A workspace whose `app:bench`, tagged `bench`, and `app:test`, tagged
/// `check-fast`, each keep their environment in the run's scratch
/// directory and try to write in `cache`; and a host that declares
/// `cache` with one variable.
fn cold_workspace(name: &str) -> (Repo, support::Scratch, Checks) {
    let repo = Repo::new(name);
    let host = support::Scratch::new(&format!("{name}-host"));
    let cache = host.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    repo.write(
        ".moon/workspace.yml",
        "projects:\n  app: 'app'\nvcs:\n  client: 'git'\n  defaultBranch: 'main'\ntelemetry: false\n",
    );
    repo.write(
        "app/moon.yml",
        "tasks:\n  bench:\n    command: 'sh probe.sh bench'\n    tags: [bench]\n\
         \x20 test:\n    command: 'sh probe.sh test'\n    tags: [check-fast]\n",
    );
    repo.write(
        "app/probe.sh",
        &format!(
            "env > \"$SLUICE_TMP/$1.env\"\ntouch {}/$1 2>/dev/null\nexit 0\n",
            cache.display()
        ),
    );
    repo.write(".gitignore", ".moon/cache/\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "workspace"]);
    let c = caching(
        &repo,
        vec![HostCache {
            dir: cache,
            socket: None,
            env: [("SCCACHE_DIR".to_string(), "/caches/sccache".to_string())].into(),
        }],
    );
    (repo, host, c)
}

/// Whether each of `names`, tasks of run `req`, was given the host's
/// variable, and whether it wrote in the host's cache.
fn cold_probe(
    c: &Checks,
    req: &RunRequest,
    host: &support::Scratch,
    names: &[&str],
) -> Vec<(bool, bool)> {
    let r = finish(c, req);
    assert!(
        r.tasks.iter().all(|t| t.outcome == Outcome::Pass),
        "{:?}",
        tasks(&r)
    );
    names
        .iter()
        .map(|task| {
            let env = std::fs::read_to_string(req.place.tmp.join(format!("{task}.env"))).unwrap();
            let written = host.path().join("cache").join(task);
            let wrote = written.exists();
            let _ = std::fs::remove_file(&written);
            (
                env.lines().any(|l| l == "SCCACHE_DIR=/caches/sccache"),
                wrote,
            )
        })
        .collect()
}

/// The moon runs of `cold_workspace` under `[tests] cold = ["#bench"]`.
fn cold_request(repo: &Repo, name: &str, required: &[&str]) -> RunRequest {
    let mut req = request(repo, name, required, None);
    req.tests
        .insert("cold".into(), Setting::List(vec!["#bench".into()]));
    req.replay = false;
    req
}

/// 35: a run holding a task `[tests] cold` names by its tag is given
/// none of the host's declared variables; a run holding none of them is
/// given every one.
#[test]
fn a_cold_test_gets_no_cache() {
    let (repo, host, c) = cold_workspace("moon-cold");
    let both = cold_request(&repo, "p1", &["#bench", "#check-fast"]);
    let cold = cold_probe(&c, &both, &host, &["bench", "test"]);
    assert!(
        cold.iter().all(|(env, _)| !env),
        "every test of a cold run is run cold: {cold:?}"
    );
    let fast = cold_request(&repo, "p2", &["#check-fast"]);
    let warm = cold_probe(&c, &fast, &host, &["test"]);
    assert!(warm[0].0, "a run with no cold test got no cache");
}

/// 35: under the sandbox, a run holding a cold task cannot write the
/// host's declared cache directory; a run holding none of them can.
#[cfg(target_os = "macos")]
#[test]
fn a_cold_test_gets_no_cache_in_the_sandbox() {
    let (repo, host, c) = cold_workspace("moon-cold-sandbox");
    let sandboxed = |name: &str, required: &[&str]| {
        let mut req = cold_request(&repo, name, required);
        req.limits = Limits {
            executor: Some("sandbox".into()),
            network: Some(Network::Denied),
            memory: None,
            docker: false,
        };
        req
    };
    let cold = cold_probe(&c, &sandboxed("p1", &["#bench"]), &host, &["bench"]);
    assert_eq!(cold, [(false, false)]);
    let warm = cold_probe(&c, &sandboxed("p2", &["#check-fast"]), &host, &["test"]);
    assert_eq!(warm, [(true, true)]);
}

/// Design D9 of the change `peers` (C.6): a task's key is the sha256 of
/// its hash manifest, kept in moon's blob store under the key, and every
/// input the manifest names carries the sha256 of the file's content.
/// So a signer can check a reported key against the combination's tree
/// without running a task: the manifest hashes to the key, names the
/// task, and names inputs whose contents the tree holds; each dependency
/// is a key the same check holds for, or `ignored` when the dependency
/// has no outputs and so enters no key.
#[test]
fn a_key_is_checked_against_the_tree_without_running() {
    use sha2::{Digest, Sha256};

    let repo = workspace("moon-key-manifest");
    let c = checks(&repo);
    let report = finish(&c, &request(&repo, "p1", &["#check-fast"], None));
    assert_eq!(report.tasks.len(), 2);
    let hex = |b: &[u8]| -> String {
        Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    };
    let manifest = |key: &str| -> serde_json::Value {
        let blob = repo
            .dir()
            .join(".moon/cache/blobs")
            .join(&key[..2])
            .join(&key[2..]);
        let bytes = std::fs::read(&blob).unwrap_or_else(|_| panic!("no manifest for {key}"));
        assert_eq!(hex(&bytes), key, "the manifest hashes to its key");
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()[0].clone()
    };
    fn check(
        key: &str,
        target: Option<&str>,
        manifest: &dyn Fn(&str) -> serde_json::Value,
        tree: &dyn Fn(&str) -> String,
    ) {
        let m = manifest(key);
        if let Some(target) = target {
            assert_eq!(m["target"], target);
        }
        let inputs = m["inputs"].as_object().unwrap();
        assert!(!inputs.is_empty());
        for (path, sum) in inputs {
            assert_eq!(&tree(path), sum, "{path} as the tree holds it");
        }
        for (dep, k) in m["deps"].as_object().unwrap() {
            match k.as_str().unwrap() {
                "ignored" => {}
                k => check(k, Some(dep), manifest, tree),
            }
        }
    }
    let tree = |path: &str| -> String {
        let out = repo.git_raw(&["cat-file", "blob", &format!("HEAD:{path}")]);
        assert!(out.status.success(), "{path} is in the tree");
        hex(&out.stdout)
    };
    for t in &report.tasks {
        check(&t.hash, Some(&t.task), &manifest, &tree);
    }
    let beta = report.tasks.iter().find(|t| t.task == "beta:test").unwrap();
    assert_eq!(manifest(&beta.hash)["deps"]["alpha:test"], "ignored");
}

/// Design D9: the profile's `proven` keeps every task whose key the
/// candidate's tree bears out, and drops one whose manifest was changed
/// after the run, as a check that lies about its key would leave it.
/// Design D5 of `a-signed-run-without-a-sandbox-proves-its-passes`, the
/// pass-proof spec's "The check profile says why": the dropped task is
/// answered among the refused with its key and the words `check_key`
/// gave.
#[test]
fn proven_drops_a_key_the_tree_does_not_bear_out() {
    let repo = workspace("moon-proven");
    let c = checks(&repo);
    let (run, report) = finish_run(&c, &request(&repo, "p1", &["#check-fast"], None));
    let mut all = c.proven(&run, &report).unwrap().proven;
    all.sort();
    assert_eq!(all, ["alpha:test", "beta:test"]);
    let alpha = report
        .tasks
        .iter()
        .find(|t| t.task == "alpha:test")
        .unwrap();
    let blob = repo
        .dir()
        .join(".moon/cache/blobs")
        .join(&alpha.hash[..2])
        .join(&alpha.hash[2..]);
    let forged = std::fs::read_to_string(&blob)
        .unwrap()
        .replace("alpha/value.txt", "alpha/other.txt");
    std::fs::write(&blob, forged).unwrap();
    let proof = c.proven(&run, &report).unwrap();
    assert_eq!(proof.proven, ["beta:test"]);
    assert_eq!(proof.unkept, []);
    let words = sluice::profiles::moon::check_key(
        &alpha.hash,
        "alpha:test",
        &|k| sluice::profiles::moon::blob_in(&repo.dir().join(".moon/cache"), k),
        &|p| sluice::profiles::moon::file_at(repo.dir(), "HEAD", p),
    )
    .unwrap_err();
    assert_eq!(
        proof.refused,
        [Refused {
            task: "alpha:test".into(),
            key: alpha.hash.clone(),
            why: format!("{words:#}"),
        }]
    );
    assert!(
        proof.refused[0].why.starts_with("alpha:test: "),
        "{proof:?}"
    );
}

/// Record 0105, design D2 to D4 of the change
/// `a-pass-nothing-proves-blames-no-one` (114): a pass whose manifest is
/// gone from the store the profile reads is neither proven nor dropped
/// as a key the tree does not bear out. It is unkept, saying why and
/// naming the store, read from the file's absence alone. Design D3 of
/// `a-signed-run-without-a-sandbox-proves-its-passes`, the pass-proof
/// spec's "A manifest gone from the store": it names the missing
/// manifest's key, and what the store says of itself, which kept
/// `beta:test`'s.
#[test]
fn a_pass_whose_manifest_is_gone_is_unkept_naming_the_store() {
    let repo = workspace("moon-unkept");
    let c = checks(&repo);
    let (run, report) = finish_run(&c, &request(&repo, "p1", &["#check-fast"], None));
    let alpha = report
        .tasks
        .iter()
        .find(|t| t.task == "alpha:test")
        .unwrap();
    assert_eq!(alpha.outcome, Outcome::Pass);
    let store = repo.dir().join(".moon/cache");
    std::fs::remove_file(
        store
            .join("blobs")
            .join(&alpha.hash[..2])
            .join(&alpha.hash[2..]),
    )
    .unwrap();
    let proof = c.proven(&run, &report).unwrap();
    assert_eq!(proof.proven, ["beta:test"]);
    assert_eq!(
        proof.unkept,
        [NotRun {
            task: "alpha:test".into(),
            reason: format!(
                "it passed, but nothing the run kept proves it ran: \
                 moon's manifest of it is not in {}, \
                 where moon kept others of the run's manifests",
                store.display()
            ),
            key: Some(alpha.hash.clone()),
        }]
    );
}

/// Design D3 of the change `a-pass-nothing-proves-blames-no-one` (114):
/// only a pass is unkept. A check that failed and whose manifest is gone
/// from the store is named neither proven nor unkept, read as before.
#[test]
fn a_failed_task_whose_manifest_is_gone_is_not_unkept() {
    let repo = workspace("moon-failed-unkept");
    change(
        &repo,
        &[(
            "gamma/moon.yml",
            "tasks:\n  test:\n    command: 'sh -c \"exit 1\"'\n    inputs: ['value.txt']\n    tags: [check-tools]\n",
        )],
    );
    let c = checks(&repo);
    let (run, report) = finish_run(
        &c,
        &request(&repo, "p1", &["#check-fast", "gamma:test"], None),
    );
    let gamma = report
        .tasks
        .iter()
        .find(|t| t.task == "gamma:test")
        .unwrap();
    assert_eq!(gamma.outcome, Outcome::Fail);
    assert!(matches!(gamma.source, Source::Ran { .. }));
    let blob = repo
        .dir()
        .join(".moon/cache/blobs")
        .join(&gamma.hash[..2])
        .join(&gamma.hash[2..]);
    if blob.exists() {
        std::fs::remove_file(&blob).unwrap();
    }
    let proof = c.proven(&run, &report).unwrap();
    assert!(!proof.proven.iter().any(|t| t == "gamma:test"), "{proof:?}");
    assert!(
        !proof.unkept.iter().any(|n| n.task == "gamma:test"),
        "{proof:?}"
    );
    let mut proven = proof.proven.clone();
    proven.sort();
    assert_eq!(proven, ["alpha:test", "beta:test"]);
}

/// ADR 0050: before any run, moon in the worktree names each required
/// task it keeps out of CI, with the entry that requires it: one by its
/// name, one through a tag. A task in CI is not named, and nothing runs.
#[test]
fn a_required_task_kept_out_of_ci_is_named_before_any_run() {
    let repo = workspace("moon-kept-out");
    repo.write(
        "gamma/moon.yml",
        "tasks:\n  test:\n    command: 'true'\n    inputs: ['value.txt']\n    tags: [check-tools]\n  \
         serve:\n    command: 'touch served'\n    tags: [check-tools]\n    options:\n      runInCI: false\n  \
         integration:\n    command: 'touch integrated'\n    options:\n      runInCI: skip\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "kept out"]);
    let c = checks(&repo);
    let required = [
        "#check-fast".to_string(),
        "#check-tools".to_string(),
        "gamma:integration".to_string(),
    ];
    let kept: Vec<(String, String)> = c
        .kept_out(repo.dir(), &moon(), &required)
        .unwrap()
        .into_iter()
        .map(|k| (k.task, k.entry))
        .collect();
    assert_eq!(
        kept,
        [
            (
                "gamma:integration".to_string(),
                "gamma:integration".to_string()
            ),
            ("gamma:serve".to_string(), "#check-tools".to_string()),
        ]
    );
    assert!(!repo.dir().join("gamma/served").exists(), "nothing ran");
    assert!(!repo.dir().join("gamma/integrated").exists(), "nothing ran");
}

/// ADR 0050: asking moon, with the repository's toolchain set up, leaves
/// the worktree as it was, so the next submit from it is not refused as
/// holding more than its commits (2); the toolchain's script is kept in
/// sluice's state, and gone once moon has answered.
#[test]
fn asking_moon_leaves_the_worktree_clean() {
    let repo = workspace("moon-kept-out-clean");
    let c = checks(&repo);
    let mut tests = moon();
    tests.insert(
        "toolchain".into(),
        Setting::List(vec!["echo".into(), "export SLUICE_TOOLCHAIN=1".into()]),
    );
    let kept = c
        .kept_out(repo.dir(), &tests, &["#check-fast".to_string()])
        .unwrap();
    assert!(kept.is_empty(), "{kept:?}");
    assert_eq!(
        repo.git(&["status", "--porcelain", "--untracked-files=all"]),
        ""
    );
    let runs = repo.dir().join(".git/sluice/.runs");
    let left: Vec<_> = std::fs::read_dir(&runs)
        .map(|d| d.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

/// Design D8 on the fleet's layout: a bare `<repo>.git` with its worktree
/// beside it. moon itself, with its shared worktree setting on and a
/// home of the test's own, keeps the task's manifest in `cache/shared`
/// under that home and not in the worktree, which is where `store` finds
/// it, and the key is proven reading it there.
#[test]
fn a_key_run_in_a_bare_repositorys_worktree_is_proven() {
    use sluice::profiles::moon::{blob_in, check_key, file_at, store};

    let src = workspace("moon-bare-src");
    let scratch = support::Scratch::new("moon-bare");
    let bare = scratch.path().join("repo.git");
    let worktree = scratch.path().join("main");
    let home = scratch.path().join("moon-home");
    src.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    src.git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "main",
    ]);

    // Everything moon is given, on its process alone: no `MOON_*` of the
    // test's own, nor its home. The path, `HOME` and `PROTO_HOME` are kept,
    // as a run keeps them, so proto's shim finds the moon installed.
    let mut env: std::collections::BTreeMap<String, String> = ["PATH", "HOME", "PROTO_HOME"]
        .into_iter()
        .filter_map(|k| Some((k.to_string(), std::env::var(k).ok()?)))
        .collect();
    env.extend(
        [
            ("MOON_HOME", home.display().to_string()),
            ("MOON_CACHE_SHARED_WORKTREE_CACHE", "true".into()),
            ("MOON_EXPERIMENT_CAS_OUTPUTS_CACHE", "true".into()),
        ]
        .map(|(k, v)| (k.to_string(), v)),
    );
    let out = support::isolated(std::process::Command::new("moon").env_clear())
        .current_dir(&worktree)
        .envs(&env)
        .args(["exec", "alpha:test", "--ci"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "moon: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(worktree.join(".moon/cache/runReport.json")).unwrap(),
    )
    .unwrap();
    let key = report["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["node"]["params"]["target"] == "alpha:test")
        .flat_map(|a| a["operations"].as_array().unwrap())
        .find(|o| o["meta"]["type"] == "hash-generation")
        .and_then(|o| o["meta"]["hash"].as_str())
        .expect("the task's key in the run report")
        .to_string();

    let at = store(&worktree, &env);
    assert_eq!(at, home.join("cache/shared"));
    let manifest = |root: &std::path::Path| root.join("blobs").join(&key[..2]).join(&key[2..]);
    assert!(manifest(&at).is_file(), "the manifest under moon's home");
    assert!(
        !manifest(&worktree.join(".moon/cache")).exists(),
        "no manifest in the worktree"
    );
    check_key(&key, "alpha:test", &|k| blob_in(&at, k), &|p| {
        file_at(&worktree, "HEAD", p)
    })
    .unwrap();

    // moon fetched nothing into its fresh home. Beside its cache it keeps
    // its id, an empty `temp`, and the system toolchain's plugin, which
    // moon carries in its own binary and writes out (moon 2.6.1,
    // `crates/config-loader/src/toolchains_config_ext.rs`).
    fn files(dir: &std::path::Path, under: &str, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let name = format!("{under}{}", e.file_name().to_string_lossy());
            if e.file_type().unwrap().is_dir() {
                if name != "cache" {
                    files(&e.path(), &format!("{name}/"), out);
                }
            } else {
                out.push(name);
            }
        }
    }
    let mut held = Vec::new();
    files(&home, "", &mut held);
    held.sort();
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(held[0], "id", "{held:?}");
    assert!(
        held[1].starts_with("plugins/toolchains/system-") && held[1].ends_with(".wasm"),
        "{held:?}"
    );
}

/// Design D8 (C.2 moon, 35): moon's home is read as the run's command
/// starts in it, the toolchain sourced. On the fleet's layout, a toolchain
/// that sets `MOON_HOME` and moon's shared worktree setting, as sluice's
/// own devenv sets its home, has moon keep the run's manifests under that
/// home, whatever home sluice's own process has; and the run's keys are
/// proven reading them there.
#[test]
fn a_key_is_read_under_the_home_the_toolchain_gives_moon() {
    let src = workspace("moon-toolchain-home-src");
    let scratch = support::Scratch::new("moon-toolchain-home");
    let bare = scratch.path().join("repo.git");
    let worktree = scratch.path().join("main");
    let home = scratch.path().join("moon-home");
    src.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    src.git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "main",
    ]);
    let mut req = request(&src, "p1", &["alpha:test"], None);
    req.place.path = worktree.clone();
    req.tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "echo".into(),
            format!(
                "export MOON_HOME='{}' MOON_CACHE_SHARED_WORKTREE_CACHE=true \
                 MOON_EXPERIMENT_CAS_OUTPUTS_CACHE=true",
                home.display()
            ),
        ]),
    );
    let c = checks_in(&bare, Vec::new());
    let (run, report) = finish_run(&c, &req);
    assert_eq!(
        tasks(&report),
        [("alpha:test".into(), Outcome::Pass, false)]
    );

    let key = &report.tasks[0].hash;
    let manifest = |root: &Path| root.join("blobs").join(&key[..2]).join(&key[2..]);
    assert!(
        manifest(&home.join("cache/shared")).is_file(),
        "the manifest under the toolchain's home"
    );
    assert!(
        !manifest(&worktree.join(".moon/cache")).exists(),
        "no manifest in the worktree"
    );
    assert_eq!(c.proven(&run, &report).unwrap().proven, ["alpha:test"]);
}

/// C.2 moon row, the pass-proof spec's "An install and a check depending
/// on it" (design of `a-signed-run-without-a-sandbox-proves-its-passes`,
/// "What moon 2.6.1 does"): on the layout of
/// `a_key_is_read_under_the_home_the_toolchain_gives_moon`, a task moon
/// does not cache, with no outputs, and a check depending on it, run with
/// no replay, each have their manifest in `cache/shared` under the
/// toolchain's moon home; the check's names the uncached task `ignored`,
/// and both are proven.
#[test]
fn a_task_moon_does_not_cache_and_its_dependent_are_proven() {
    let src = workspace("moon-uncached-src");
    change(
        &src,
        &[(
            "alpha/moon.yml",
            "tasks:\n  install:\n    command: 'true'\n    options:\n      cache: false\n  \
             test:\n    command: 'sh test.sh'\n    inputs: ['test.sh', 'value.txt', 'notes.md']\n    \
             deps: ['alpha:install']\n    tags: [check-fast]\n",
        )],
    );
    let scratch = support::Scratch::new("moon-uncached");
    let bare = scratch.path().join("repo.git");
    let worktree = scratch.path().join("main");
    let home = scratch.path().join("moon-home");
    src.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    src.git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "main",
    ]);
    let mut req = request(&src, "p1", &["alpha:install", "alpha:test"], None);
    req.place.path = worktree.clone();
    req.replay = false;
    req.tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "echo".into(),
            format!(
                "export MOON_HOME='{}' MOON_CACHE_SHARED_WORKTREE_CACHE=true \
                 MOON_EXPERIMENT_CAS_OUTPUTS_CACHE=true",
                home.display()
            ),
        ]),
    );
    let c = checks_in(&bare, Vec::new());
    let (run, report) = finish_run(&c, &req);
    let mut ran = tasks(&report);
    ran.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        ran,
        [
            ("alpha:install".into(), Outcome::Pass, false),
            ("alpha:test".into(), Outcome::Pass, false),
        ]
    );

    let shared = home.join("cache/shared");
    let manifest = |key: &str| -> serde_json::Value {
        let at = shared.join("blobs").join(&key[..2]).join(&key[2..]);
        let bytes =
            std::fs::read(&at).unwrap_or_else(|_| panic!("no manifest at {}", at.display()));
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()[0].clone()
    };
    let key = |task: &str| {
        report
            .tasks
            .iter()
            .find(|t| t.task == task)
            .unwrap()
            .hash
            .clone()
    };
    assert_eq!(manifest(&key("alpha:install"))["target"], "alpha:install");
    assert_eq!(
        manifest(&key("alpha:test"))["deps"]["alpha:install"],
        "ignored"
    );
    let mut proven = c.proven(&run, &report).unwrap().proven;
    proven.sort();
    assert_eq!(proven, ["alpha:install", "alpha:test"]);
}

/// Design D1 of the change `a-signed-run-without-a-sandbox-proves-its-
/// passes` (C.2 moon row; rule core.9), the pass-proof spec's "moon kept
/// the manifest in the checkout" and "Neither store holds it": on the
/// layout of `a_key_is_read_under_the_home_the_toolchain_gives_moon`, a
/// manifest moon kept in the checkout's own `.moon/cache` and not in the
/// shared store the run's record names proves its key; one in neither is
/// a pass the run kept no proof of, naming the shared store.
#[test]
fn a_manifest_moon_kept_in_the_checkout_proves_its_key() {
    let src = workspace("moon-checkout-store-src");
    let scratch = support::Scratch::new("moon-checkout-store");
    let bare = scratch.path().join("repo.git");
    let worktree = scratch.path().join("main");
    let home = scratch.path().join("moon-home");
    src.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    src.git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "main",
    ]);
    let mut req = request(&src, "p1", &["alpha:test"], None);
    req.place.path = worktree.clone();
    req.tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "echo".into(),
            format!(
                "export MOON_HOME='{}' MOON_CACHE_SHARED_WORKTREE_CACHE=true \
                 MOON_EXPERIMENT_CAS_OUTPUTS_CACHE=true",
                home.display()
            ),
        ]),
    );
    let c = checks_in(&bare, Vec::new());
    let (run, report) = finish_run(&c, &req);
    assert_eq!(
        tasks(&report),
        [("alpha:test".into(), Outcome::Pass, false)]
    );

    let key = &report.tasks[0].hash;
    let shared = home.join("cache/shared");
    let manifest = |root: &Path| root.join("blobs").join(&key[..2]).join(&key[2..]);
    let own = manifest(&worktree.join(".moon/cache"));
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::rename(manifest(&shared), &own).unwrap();
    assert!(
        !manifest(&shared).exists(),
        "no manifest in the shared store"
    );
    let proof = c.proven(&run, &report).unwrap();
    assert_eq!(proof.proven, ["alpha:test"]);
    assert_eq!(proof.unkept, []);

    std::fs::remove_file(&own).unwrap();
    let proof = c.proven(&run, &report).unwrap();
    assert_eq!(proof.proven, Vec::<String>::new());
    assert_eq!(proof.unkept.len(), 1, "{proof:?}");
    assert_eq!(proof.unkept[0].task, "alpha:test");
    assert!(
        proof.unkept[0].reason.contains(&format!(
            "moon's manifest of it is not in {}",
            shared.display()
        )),
        "{proof:?}"
    );
}

/// Record 0105 point 1 (C.2 moon and sandbox rows as amended at
/// agentplot/blueprints 0c38f24; 108; rules core.9, core.35), the
/// host-caches spec's "moon's home moved by the toolchain, on a host that
/// shares moon's cache": `a_key_is_read_under_the_home_the_toolchain_gives_moon`
/// under the sandbox, kept off the network. The host's cache, at a
/// directory that is not moon's home, turns moon's sharing on; the
/// toolchain moves moon's home under `/tmp`, reached through a link, where
/// moon has not made `cache/shared`. The sandbox opens the store the run
/// records, so moon keeps the task's manifest there and its key is proven.
#[cfg(target_os = "macos")]
#[test]
fn a_key_run_in_the_sandbox_is_proven_under_the_home_the_toolchain_gives_moon() {
    let src = workspace("moon-sandbox-home-src");
    let scratch = support::Scratch::new("moon-sandbox-home");
    let bare = scratch.path().join("repo.git");
    let worktree = scratch.path().join("main");
    let cache = scratch.path().join("host-cache");
    std::fs::create_dir_all(&cache).unwrap();
    // moon's home by a path under `/tmp`, a link to `/private/tmp`, as the
    // toolchain gives it; none of it made by the test.
    let linked = support::Scratch(PathBuf::from(format!(
        "/tmp/sluice-test-{}-moon-sandbox-home",
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(linked.path());
    std::fs::create_dir_all(linked.path()).unwrap();
    assert_ne!(
        std::fs::canonicalize(linked.path()).unwrap(),
        linked.path(),
        "the home is reached through a link"
    );
    let home = linked.path().join("moon-home");
    src.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    src.git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "main",
    ]);
    let mut req = request(&src, "p1", &["alpha:test"], None);
    req.place.path = worktree.clone();
    req.limits = Limits {
        executor: Some("sandbox".into()),
        network: Some(Network::Denied),
        memory: None,
        docker: false,
    };
    req.tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "echo".into(),
            format!("export MOON_HOME='{}'", home.display()),
        ]),
    );
    let c = checks_in(
        &bare,
        vec![HostCache {
            dir: cache.clone(),
            socket: None,
            env: [
                ("MOON_CACHE_SHARED_WORKTREE_CACHE", "true"),
                ("MOON_EXPERIMENT_CAS_OUTPUTS_CACHE", "true"),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into(),
        }],
    );
    let (run, report) = finish_run(&c, &req);
    assert_eq!(
        tasks(&report),
        [("alpha:test".into(), Outcome::Pass, false)]
    );
    assert_eq!(report.executor.kind, "sandbox");

    let key = &report.tasks[0].hash;
    let manifest = |root: &Path| root.join("blobs").join(&key[..2]).join(&key[2..]);
    assert!(
        manifest(&home.join("cache/shared")).is_file(),
        "the manifest under the toolchain's home"
    );
    assert!(
        !manifest(&worktree.join(".moon/cache")).exists(),
        "no manifest in the worktree"
    );
    assert_eq!(c.proven(&run, &report).unwrap().proven, ["alpha:test"]);
}
