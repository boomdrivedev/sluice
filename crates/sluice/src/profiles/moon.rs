//! The `moon` runner: which required tasks a candidate affects, `moon
//! exec` of exactly those in its place, and what moon's run report says
//! of each (C.2; 15, 16, 36 to 38, 70; rules core.7, core.9;
//! `docs/plans/phase-1-integrations.md` section 3). moon 2.6 or later.
//!
//! The project and task graph is read from the place, which holds the
//! candidate; every moon command gets the run's environment, so nothing of
//! `MOON_*` in sluice's own reaches it, and stdin from `/dev/null`, but
//! the affected reading of a run's plan, which is handed the files the run
//! changed on its stdin (36; design D3 of
//! a-worktrees-checks-run-only-what-its-own-changes-affect).
//!
//! A required task moon skipped is read through the dependencies the plan
//! kept (149; C.2). moon's run report names no edge of its graph, so the
//! plan keeps each target's dependencies at any depth from the `moon query
//! tasks` it reads, leaving out a `cleanup` one, which moon runs after its
//! task and never skips the task for. A target moon skipped for a task it
//! depends on that ran and failed is failed by that task, whose result the
//! report gives under its own key; one skipped for a task that exited as
//! `[tests] not_run` names is not run (114); any other is left without a
//! result (15). In a run its time limit ended nothing is read through a
//! dependency (66).
//!
//! Before any moon command, the working copy is equipped by the check
//! profile's one sequence (147; rule core.46; [`super::checks::Equipping`]):
//! the repository's toolchain, then the steps [`Moon::steps`] gives, each
//! outside the executor with the repository's toolchain sourced: `proto
//! install` where the working copy has a `.prototools` (the file listing
//! the exact version of each tool the repository uses, moon among them),
//! `moon --version`, and `moon setup`. moon cannot run until it is
//! installed, and nothing is installed inside the executor. A working copy
//! moon is only asked a question in is equipped up to the version check;
//! one moon runs a task in, through the setup.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::Value;
use sluice_seams::{
    EquipStep, FailedBy, KeptOutTask, NotRun, Outcome, Reads, Rev, RunDetail, Source, TaskProgress,
    TaskResult, TaskState, Through,
};

use super::checks::{cold_tests, failed, not_run_reason, passed, printed, why, Context, Plan};

/// The run report, relative to the place.
const REPORT: &str = ".moon/cache/runReport.json";

/// What starts each of moon's checkpoint lines in a run's output.
const CHECKPOINT: &str = "\u{25AE}\u{25AE}\u{25AE}\u{25AE} ";

/// The oldest moon the runner reads: 2.6 added `wait` and `cleanup`
/// dependencies, which a repository's checks use to bring up what they
/// need (31).
const FLOOR: (u64, u64) = (2, 6);

pub struct Moon;

/// 147: one of the runner's steps of equipping a working copy, with how
/// far an equip goes before it is run.
pub struct Step {
    pub through: Through,
    pub run: fn(&Moon, &Context) -> EquipStep,
}

/// A task as `moon query tasks` lists it.
struct Task {
    tags: Vec<String>,
    /// Whether moon runs it in its CI mode (`runInCI`).
    in_ci: bool,
    /// 149; C.2: the tasks it depends on directly, every one of its
    /// `deps` but a `cleanup` one, which moon runs after it and never
    /// skips it for.
    deps: Vec<String>,
}

/// 149; design D2: what a run's report says of its required checks.
#[derive(Debug, Default)]
pub struct Read {
    /// The results of the checks the run judged (15, 16, 38).
    pub tasks: Vec<TaskResult>,
    /// The checks it did not judge (could-not-run-1 114), a check whose
    /// dependency could not run among them.
    pub not_run: Vec<NotRun>,
    /// The result of each task a check of `failed_by` was failed by that
    /// is no check of the run, each once.
    pub dependencies: Vec<TaskResult>,
    /// The checks moon skipped for a task they depend on that ran and
    /// failed.
    pub failed_by: Vec<FailedBy>,
    /// What moon says of the run (119).
    pub detail: Option<RunDetail>,
}

impl Moon {
    pub fn plan(&self, cx: &Context) -> Result<Plan> {
        let request = cx.request;
        let place = &request.place.path;
        let cold = cold_tests(&request.tests)?;
        let tasks = self.tasks(cx, Affected::All)?;
        let mut wanted = BTreeSet::new();
        for r in &request.required {
            match r.strip_prefix('#') {
                Some(tag) => wanted.extend(
                    tasks
                        .iter()
                        .filter(|(_, task)| task.tags.iter().any(|t| t == tag))
                        .map(|(t, _)| t.clone()),
                ),
                None => {
                    wanted.insert(r.clone());
                }
            }
        }
        // 36, 70: what the run changed since `since`, an ancestor of its
        // revision, read once; every required task when there is no
        // `since`, or when moon's own configuration changed, since then the
        // graph itself did. moon is handed the files rather than the range
        // (design D3).
        if let Some(since) = &request.since {
            let changed = changed_files(place, since.as_str(), request.revision.as_str())?;
            let graph_changed = changed
                .iter()
                .any(|p| p.starts_with(".moon/") || p == "moon.yml" || p.ends_with("/moon.yml"));
            if !graph_changed {
                let affected: BTreeSet<String> = self
                    .tasks(cx, Affected::Files(&changed))?
                    .into_keys()
                    .collect();
                wanted.retain(|t| affected.contains(t));
            }
        }
        let targets: Vec<String> = wanted.into_iter().collect();
        // 149: the run report names no edge of the graph, so the plan
        // keeps what each target depends on, for the reading to find the
        // task a skipped target was skipped for.
        let deps = dependencies(&tasks, &targets);
        // ADR 0044: moon runs in its CI mode, which runs no task the
        // repository keeps out of CI; each such target reads missing.
        let kept_out: Vec<String> = kept_out(&tasks, &request.required)
            .into_iter()
            .map(|k| k.task)
            .filter(|t| targets.contains(t))
            .collect();
        // A report left in the place is not this run's.
        match std::fs::remove_file(place.join(REPORT)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        if targets.is_empty() {
            return Ok(Plan {
                argv: None,
                targets,
                holds_processors: true,
                stamps: Vec::new(),
                kept_out,
                cold: false,
                deps,
            });
        }
        let cold = holds_cold(&tasks, &targets, &cold);
        let jobs = cx.jobs.to_string();
        let mut argv: Vec<&str> = vec!["moon"];
        // ADR 0014: moon exports beside sluice; it reads no parent trace,
        // so the run's resource attributes tie its spans to the round.
        if crate::telemetry::on() {
            argv.push("--otel");
        }
        argv.push("exec");
        argv.extend(targets.iter().map(String::as_str));
        argv.extend(["--ci", "--on-failure", "continue", "--concurrency", &jobs]);
        // A gate that does not trust the cache gets no replay (28).
        if !request.replay {
            argv.push("--force");
        }
        Ok(Plan {
            argv: Some(cx.argv(&argv)),
            targets,
            holds_processors: true,
            // proto stamps each toolchain it runs as used.
            stamps: vec![".last-used"],
            kept_out,
            cold,
            deps,
        })
    }

    /// ADR 0050: which of the required checks moon keeps out of its CI
    /// mode in the place, read as `plan` reads them, before any run, by
    /// `moon query tasks` in a working copy equipped up to the version
    /// check (147). Runs no task.
    pub fn kept_out(&self, cx: &Context) -> Result<Vec<KeptOutTask>> {
        let tasks = self.tasks(cx, Affected::All)?;
        Ok(kept_out(&tasks, &cx.request.required))
    }

    /// 140: for each change of `pairs`, the required tasks it affects,
    /// dependents included, read by `moon query tasks --affected` in the
    /// place, which holds the first change's second revision; every task
    /// for a change to a setting of moon's own, and for a change whose
    /// first revision differs from the place's in one, since the graph
    /// the place holds is then not that change's. Runs no task.
    pub fn reads(&self, cx: &Context, pairs: &[(Rev, Rev)]) -> Result<Vec<Reads>> {
        let place = &cx.request.place.path;
        let at = cx.request.revision.as_str();
        let tasks = self.tasks(cx, Affected::All)?;
        let mut wanted = BTreeSet::new();
        for r in &cx.request.required {
            match r.strip_prefix('#') {
                Some(tag) => wanted.extend(
                    tasks
                        .iter()
                        .filter(|(_, task)| task.tags.iter().any(|t| t == tag))
                        .map(|(t, _)| t.clone()),
                ),
                None => {
                    wanted.insert(r.clone());
                }
            }
        }
        let mut reads = Vec::new();
        for (from, to) in pairs {
            let own = |a: &str, b: &str| -> Result<bool> {
                Ok(changed_files(place, a, b)?.iter().any(|p| own_setting(p)))
            };
            if own(from.as_str(), to.as_str())? || own(from.as_str(), at)? {
                reads.push(Reads::Every);
                continue;
            }
            let affected = self.tasks(cx, Affected::Range(from.as_str(), to.as_str()))?;
            reads.push(Reads::Checks(
                wanted
                    .iter()
                    .filter(|t| affected.contains_key(*t))
                    .cloned()
                    .collect(),
            ));
        }
        Ok(reads)
    }

    /// What a task printed, as moon keeps it in the place: `stdout.log`
    /// and `stderr.log` in its state directory (`state_dir`). `None` when
    /// moon kept neither, as for a task it never ran.
    pub fn log(&self, place: &Path, target: &str) -> Option<Vec<u8>> {
        let dir = state_dir(place, target)?;
        let mut text = Vec::new();
        for stream in ["stdout", "stderr"] {
            if let Ok(b) = std::fs::read(dir.join(format!("{stream}.log"))) {
                if !b.is_empty() {
                    text.extend(format!("--- {stream} ---\n").into_bytes());
                    text.extend(b);
                    if !text.ends_with(b"\n") {
                        text.push(b'\n');
                    }
                }
            }
        }
        (!text.is_empty()).then_some(text)
    }

    /// Each required target's result from moon's run report: a task that
    /// ran, `Ran` with its outcome; one answered from the cache,
    /// `Replayed`; one that failed with an exit `not_run` names, not run
    /// (could-not-run-1 114). A target with no result of its own is read
    /// through `deps`, what the plan kept of each target's dependencies
    /// (149; design D2): failed by the tasks it depends on that ran and
    /// failed, then not run where one could not run, then no result (15,
    /// 16, 38; rule core.9). With them, what moon says of the run (119).
    pub fn read(
        &self,
        place: &Path,
        targets: &[String],
        deps: &BTreeMap<String, Vec<String>>,
        not_run: &[i32],
        host: &str,
    ) -> Result<Read> {
        let report = match std::fs::read(place.join(REPORT)) {
            Ok(b) => b,
            // Ended before moon wrote it, or nothing was run.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Read::default()),
            Err(e) => return Err(e.into()),
        };
        let report: Value = serde_json::from_slice(&report).context("reading moon's run report")?;
        Ok(read_report(&report, targets, deps, not_run, host))
    }

    /// ADR 0028: where each of `targets` has got in a run still going,
    /// started at `started_ms` (milliseconds since the epoch), from what
    /// moon writes as it goes. A task's end is its run record,
    /// `lastRun.json` in its state directory (`state_dir`) in the place,
    /// written at or after the run's start (C.2); its start, its time and
    /// whether it was answered from the cache are moon's checkpoints in
    /// the run's output. A running task started when its start checkpoint
    /// was printed: the first of `times`, the output's length and the
    /// time as it grew, to reach that line's end (ADR 0042). Never a
    /// result: the run report is (37, 38).
    pub fn progress(
        &self,
        place: &Path,
        output: &[u8],
        times: &[(u64, String)],
        targets: &[String],
        started_ms: i64,
    ) -> Vec<TaskProgress> {
        let marks = checkpoints(output);
        targets
            .iter()
            .map(|target| {
                let mark = marks.get(target.as_str());
                let ended = last_run(place, target).filter(|r| r.at >= started_ms);
                let Some(r) = ended else {
                    let printed = mark.and_then(|m| m.start).and_then(|end| {
                        times
                            .iter()
                            .find(|(len, _)| *len >= end)
                            .and_then(|(_, at)| chrono::DateTime::parse_from_rfc3339(at).ok())
                            .map(|at| timestamp(at.timestamp_millis()))
                    });
                    return TaskProgress {
                        target: target.clone(),
                        state: if mark.is_some() {
                            TaskState::Running
                        } else {
                            TaskState::Waiting
                        },
                        started: printed,
                        ended: None,
                        exit: None,
                    };
                };
                let cached = mark.is_some_and(|m| m.cached);
                let state = match (cached, r.exit) {
                    (true, _) => TaskState::Replayed,
                    (false, 0) => TaskState::Passed,
                    (false, _) => TaskState::Failed,
                };
                let took = mark.and_then(|m| m.took).filter(|_| !cached);
                TaskProgress {
                    target: target.clone(),
                    state,
                    started: took.map(|ms| timestamp(r.at - ms)),
                    ended: Some(timestamp(r.at)),
                    exit: (!cached).then_some(r.exit),
                }
            })
            .collect()
    }

    /// Time-limit-1 66: which of `targets` the time limit cut off, in a run
    /// started at `started_ms` that it ended at `limit_ms` (milliseconds
    /// since the epoch). One cut off started in the run, by its result,
    /// its run record or its start checkpoint, and has no run record from
    /// before the limit: moon reads one it ended as failed when it caught
    /// the signal and exited, as `bun test` does, and as skipped when it
    /// died of it. A replay ran nothing, so none is cut off.
    pub fn cut_off(
        &self,
        place: &Path,
        output: &[u8],
        targets: &[String],
        results: &[TaskResult],
        started_ms: i64,
        limit_ms: i64,
    ) -> Vec<String> {
        let marks = checkpoints(output);
        targets
            .iter()
            .filter(|target| {
                let result = results.iter().find(|r| &r.task == *target);
                if result.is_some_and(|r| matches!(r.source, Source::Replayed { .. })) {
                    return false;
                }
                let ended = last_run(place, target)
                    .map(|r| r.at)
                    .filter(|at| *at >= started_ms);
                if ended.is_some_and(|at| at < limit_ms) {
                    return false;
                }
                result.is_some() || ended.is_some() || marks.contains_key(target.as_str())
            })
            .cloned()
            .collect()
    }

    /// 147: the runner's steps of equipping `working_copy`, in the order
    /// they run, after the toolchain: `proto install` where it has a
    /// `.prototools`, and `moon --version`, before moon is asked anything;
    /// then `moon setup`, before moon runs a task.
    pub fn steps(&self, working_copy: &Path) -> Vec<Step> {
        let mut steps = Vec::new();
        if working_copy.join(".prototools").exists() {
            steps.push(Step {
                through: Through::Reading,
                run: Moon::install,
            });
        }
        steps.push(Step {
            through: Through::Reading,
            run: Moon::check_version,
        });
        steps.push(Step {
            through: Through::Running,
            run: Moon::setup,
        });
        steps
    }

    /// The tool versions the place's `.prototools` lists, installed. Only
    /// the repository's own file is read, not the user's global one.
    fn install(&self, cx: &Context) -> EquipStep {
        let argv = ["proto", "install", "--config-mode", "upwards"];
        self.step(cx, "proto install", &argv).0
    }

    /// The workspace's toolchains, installed in the place, outside the
    /// executor, where a download and a write to the user's home are
    /// allowed: the run inside only uses them.
    fn setup(&self, cx: &Context) -> EquipStep {
        self.step(cx, "moon setup", &["moon", "setup"]).0
    }

    /// 147: `argv` run as [`Moon::run`] runs it, as the step `name`, with
    /// what it printed where it could be run.
    fn step(&self, cx: &Context, name: &str, argv: &[&str]) -> (EquipStep, Option<Output>) {
        let out = match self.spawn(cx, argv, &[], None) {
            Ok(out) => out,
            Err(e) => return (failed(name, &format!("could not be run: {e}"), ""), None),
        };
        let said = printed(&[&out.stdout, &out.stderr]);
        let step = match why(out.status) {
            None => passed(name, said),
            Some(reason) => failed(name, &reason, &said),
        };
        (step, Some(out))
    }

    fn command(
        &self,
        cx: &Context,
        args: &[&str],
        env: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut argv = vec!["moon"];
        argv.extend(args);
        self.run(cx, &argv, env, input)
    }

    fn run(
        &self,
        cx: &Context,
        argv: &[&str],
        env: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let out = self
            .spawn(cx, argv, env, input)
            .with_context(|| format!("running {}", cx.argv(argv).join(" ")))?;
        if !out.status.success() {
            let argv = cx.argv(argv);
            bail!(
                "{} exited {}: {}",
                argv.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }

    /// `argv` in the place with the toolchain sourced, the run's
    /// environment and `env` its own, and stdin `input` where it is given,
    /// else `/dev/null`.
    fn spawn(
        &self,
        cx: &Context,
        argv: &[&str],
        env: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> std::io::Result<Output> {
        let argv = cx.argv(argv);
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .current_dir(&cx.request.place.path)
            .env_clear()
            .envs(cx.env)
            .envs(env.iter().copied());
        let Some(input) = input else {
            return command.stdin(Stdio::null()).output();
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        // Written beside the read of its output, so a long list never
        // waits on a pipe moon has not read.
        std::thread::scope(|s| {
            let writer = s.spawn(move || {
                use std::io::Write as _;
                stdin.write_all(input)
            });
            let out = child.wait_with_output();
            let written = writer.join().expect("the writer of moon's stdin");
            let out = out?;
            // A moon that failed says why in what it printed; one that
            // answered without reading the whole list answered for less.
            match written {
                Err(e) if out.status.success() => Err(e),
                _ => Ok(out),
            }
        })
    }

    /// The moon the toolchain puts first, refused below `FLOOR`. Its
    /// reasons are its own and quote what it printed on its output, so
    /// only its error stream follows them.
    fn check_version(&self, cx: &Context) -> EquipStep {
        const NAME: &str = "moon --version";
        let (step, out) = self.step(cx, NAME, &["moon", "--version"]);
        let Some(out) = out.filter(|_| step.passed) else {
            return step;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let text = text.trim();
        let reason = match parse_version(text) {
            None => format!("moon --version said {text:?}"),
            Some(version) if version < FLOOR => format!(
                "{text} is below moon {}.{}, the oldest the moon runner reads",
                FLOOR.0, FLOOR.1
            ),
            Some(_) => return step,
        };
        failed(NAME, &reason, &printed(&[&out.stderr]))
    }

    /// The tasks moon knows in the place, with their tags, or those
    /// `affected` names as affected, dependents included.
    fn tasks(&self, cx: &Context, affected: Affected) -> Result<BTreeMap<String, Task>> {
        let mut args = vec!["query", "tasks"];
        if !matches!(affected, Affected::All) {
            args.extend(["--affected", "--downstream", "deep"]);
        }
        let out = match affected {
            Affected::All => self.command(cx, &args, &[], None)?,
            Affected::Range(base, head) => {
                self.command(cx, &args, &[("MOON_BASE", base), ("MOON_HEAD", head)], None)?
            }
            // moon reads no file it is handed from an empty list, as from
            // an empty diff; it is asked nothing.
            Affected::Files([]) => return Ok(BTreeMap::new()),
            Affected::Files(files) => {
                let input = serde_json::to_vec(&serde_json::json!({ "files": files }))?;
                self.command(cx, &args, &[], Some(&input))?
            }
        };
        let json: Value = serde_json::from_slice(&out).context("reading moon query tasks")?;
        Ok(query_tasks(&json))
    }
}

/// Which of moon's tasks [`Moon::tasks`] reads.
enum Affected<'a> {
    /// Every task moon knows.
    All,
    /// Those the change from the first revision to the second affects, by
    /// `MOON_BASE` and `MOON_HEAD` (140).
    Range(&'a str, &'a str),
    /// Those the files a run changed affect, handed to moon on its stdin
    /// as `{"files": [...]}` (36; design D3).
    Files(&'a [String]),
}

/// Every task a `moon query tasks` document lists, by its target.
fn query_tasks(json: &Value) -> BTreeMap<String, Task> {
    let mut tasks = BTreeMap::new();
    for project in json["tasks"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
    {
        for task in project.as_object().into_iter().flat_map(|o| o.values()) {
            let Some(target) = task["target"].as_str() else {
                continue;
            };
            let tags = task["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str().map(String::from))
                .collect();
            // `runInCI` is a boolean or, since moon 2, a word: `skip`
            // and `false` keep a task out of CI; any other runs it.
            let run_in_ci = &task["options"]["runInCI"];
            let in_ci = !(run_in_ci == &Value::Bool(false) || run_in_ci.as_str() == Some("skip"));
            let deps = task["deps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(dependency)
                .collect();
            tasks.insert(target.to_string(), Task { tags, in_ci, deps });
        }
    }
    tasks
}

/// 149; C.2: the task one item of a task's `deps` names, as `moon query
/// tasks` prints it: a target, or an object's `target`. `None` for a
/// `cleanup` one, which moon runs after its task and never skips the task
/// for, and for an item of any other shape, which names nothing sluice
/// reads, so a target skipped for it stays missing (15).
fn dependency(item: &Value) -> Option<String> {
    match item {
        Value::String(target) => Some(target.clone()),
        Value::Object(o) if o.get("type").and_then(Value::as_str) != Some("cleanup") => {
            o.get("target")?.as_str().map(String::from)
        }
        _ => None,
    }
}

/// 149; design D1: each of `targets`' dependencies at any depth of the
/// graph, each once, sorted; a target with none is left out. A cycle ends
/// where it comes back, and no target is its own dependency.
fn dependencies(
    tasks: &BTreeMap<String, Task>,
    targets: &[String],
) -> BTreeMap<String, Vec<String>> {
    targets
        .iter()
        .filter_map(|target| {
            let mut seen = BTreeSet::new();
            let mut next: Vec<&str> = vec![target];
            while let Some(t) = next.pop() {
                for d in tasks.get(t).into_iter().flat_map(|task| &task.deps) {
                    if d != target && seen.insert(d.clone()) {
                        next.push(d);
                    }
                }
            }
            (!seen.is_empty()).then(|| (target.clone(), seen.into_iter().collect()))
        })
        .collect()
}

/// ADR 0044, 0050: the tasks `required` names, by name or through a tag
/// they carry, that moon keeps out of its CI mode, each with the entry
/// that names it: its own name before the first tag. `plan` and
/// `Moon::kept_out` both read them so, so the two cannot differ.
fn kept_out(tasks: &BTreeMap<String, Task>, required: &[String]) -> Vec<KeptOutTask> {
    tasks
        .iter()
        .filter(|(_, task)| !task.in_ci)
        .filter_map(|(target, task)| {
            let entry = required.iter().find(|r| *r == target).or_else(|| {
                required.iter().find(|r| {
                    r.strip_prefix('#')
                        .is_some_and(|tag| task.tags.iter().any(|t| t == tag))
                })
            })?;
            Some(KeptOutTask {
                task: target.clone(),
                entry: entry.clone(),
            })
        })
        .collect()
}

/// 35: whether any of `targets` is a test `cold` names, by its own name
/// or by a tag it carries, as `[tests] cold` names them.
fn holds_cold(tasks: &BTreeMap<String, Task>, targets: &[String], cold: &[String]) -> bool {
    targets.iter().any(|target| {
        cold.iter().any(|c| match c.strip_prefix('#') {
            Some(tag) => tasks
                .get(target)
                .is_some_and(|task| task.tags.iter().any(|t| t == tag)),
            None => c == target,
        })
    })
}

/// 140: whether `path` is a setting of the runner's own: moon's
/// configuration, the versions proto installs, or a file the toolchain a
/// place enters is set up from, at the repository's root. A change to one
/// changes what every check runs with, so it reads every check.
fn own_setting(path: &str) -> bool {
    const TOOLCHAIN: [&str; 11] = [
        ".prototools",
        "devenv.nix",
        "devenv.yaml",
        "devenv.lock",
        "flake.nix",
        "flake.lock",
        ".envrc",
        ".tool-versions",
        "mise.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
    ];
    path.starts_with(".moon/")
        || path == "moon.yml"
        || path.ends_with("/moon.yml")
        || path.ends_with("/.prototools")
        || TOOLCHAIN.contains(&path)
}

/// `moon 2.6.1` → `(2, 6)`.
fn parse_version(text: &str) -> Option<(u64, u64)> {
    let v = text.split_whitespace().nth(1)?;
    let mut parts = v.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// A file of `revision`'s tree, read by git in the place; `None` when the
/// tree has no such file.
pub fn file_at(place: &Path, revision: &str, path: &str) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .current_dir(place)
        .args(["cat-file", "blob", &format!("{revision}:{path}")])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The files that differ between two revisions, by git, in the place.
fn changed_files(place: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .current_dir(place)
        .args(["diff", "--name-only", "-z", "--no-renames", base, head])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .output()
        .context("running git diff")?;
    if !out.status.success() {
        bail!(
            "git diff {base} {head}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect())
}

/// One task's action in a run report: a `RunTask(…)`, or a
/// `RunPersistentTask(…)`, a helper a check waits on.
struct Action<'a> {
    target: &'a str,
    persistent: bool,
    status: Option<&'a str>,
    /// moon skips no dependent for a failure it allows.
    allow_failure: bool,
    hash: Option<String>,
    hydrated: bool,
    /// The last try's exit: a retried task's last try is the one that
    /// judged it.
    exit: Option<i32>,
    tries: usize,
    took_ms: Option<u64>,
}

impl<'a> Action<'a> {
    fn of(action: &'a Value) -> Option<Action<'a>> {
        let label = action["label"].as_str()?;
        let (inner, persistent) = match label.strip_prefix("RunTask(") {
            Some(l) => (l, false),
            None => (label.strip_prefix("RunPersistentTask(")?, true),
        };
        let inner = inner.strip_suffix(')')?;
        let ops = action["operations"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let op = |kind: &str| {
            ops.iter()
                .find(|o| o["meta"]["type"].as_str() == Some(kind))
        };
        let executions = || {
            ops.iter()
                .filter(|o| o["meta"]["type"].as_str() == Some("task-execution"))
        };
        Some(Action {
            target: action["node"]["params"]["target"].as_str().unwrap_or(inner),
            persistent,
            status: action["status"].as_str(),
            allow_failure: action["allowFailure"].as_bool() == Some(true),
            hash: op("hash-generation").and_then(|o| o["meta"]["hash"].as_str().map(String::from)),
            hydrated: op("output-hydration")
                .is_some_and(|o| o["status"].as_str() == Some("cached")),
            exit: executions()
                .next_back()
                .and_then(|o| o["meta"]["exitCode"].as_i64())
                .and_then(|c| i32::try_from(c).ok()),
            tries: executions().count(),
            took_ms: report_ms(&action["duration"]),
        })
    }

    fn result(&self, hash: String, outcome: Outcome, replayed: bool, host: &str) -> TaskResult {
        TaskResult {
            took_ms: self.took_ms,
            attempts: u32::try_from(self.tries).ok().filter(|n| *n > 0),
            task: self.target.to_string(),
            hash,
            outcome,
            source: if replayed {
                Source::Replayed { from_run_at: None }
            } else {
                Source::Ran {
                    host: host.to_string(),
                }
            },
        }
    }

    /// The exit `not_run` names that this failed with, where moon read it
    /// as no failure it allows (114).
    fn could_not_run(&self, not_run: &[i32]) -> Option<i32> {
        (self.status == Some("failed") && !self.allow_failure)
            .then_some(self.exit)
            .flatten()
            .filter(|c| not_run.contains(c))
    }

    /// It ran and failed, with a hash, a failure moon allows not and one
    /// that could not run not (149).
    fn failed(&self, not_run: &[i32]) -> bool {
        self.status == Some("failed")
            && !self.allow_failure
            && self.hash.is_some()
            && self.could_not_run(not_run).is_none()
    }
}

/// What a run report says of `targets`, and of the run (119). Only
/// `RunTask(…)` actions are checks: a `wait` dependency's
/// `RunPersistentTask(…)` is a helper. A target moon skipped, with no
/// result of its own and not one that failed with an exit `not_run`
/// names, is read through its dependencies in `deps`, both kinds of
/// action alike (149; design D2): failed by those that ran and failed,
/// each that is no target giving its result to `dependencies` once;
/// otherwise not run, naming the first that could not run; otherwise left
/// without a result (15). A target the report does not name as skipped,
/// one moon keeps out of its CI mode among them (ADR 0044), was skipped
/// for nothing it depends on, and an empty `deps`, as a run an older
/// sluice started has, reads nothing through a dependency.
fn read_report(
    report: &Value,
    targets: &[String],
    deps: &BTreeMap<String, Vec<String>>,
    not_run: &[i32],
    host: &str,
) -> Read {
    let actions: Vec<Action> = report["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Action::of)
        .collect();
    let mut read = Read {
        detail: run_detail(report),
        ..Read::default()
    };
    for a in actions
        .iter()
        .filter(|a| !a.persistent && targets.iter().any(|t| t == a.target))
    {
        let Some(hash) = &a.hash else {
            continue;
        };
        let (outcome, replayed) = match a.status {
            Some("passed") => (Outcome::Pass, a.hydrated),
            Some("failed") => match a.exit.filter(|c| not_run.contains(c)) {
                Some(code) => {
                    read.not_run.push(NotRun {
                        task: a.target.to_string(),
                        reason: not_run_reason(code),
                        key: None,
                    });
                    continue;
                }
                None => (Outcome::Fail, false),
            },
            Some("cached" | "cached-from-remote") => (Outcome::Pass, true),
            _ => continue,
        };
        read.tasks
            .push(a.result(hash.clone(), outcome, replayed, host));
    }
    for target in targets {
        let judged = read.tasks.iter().any(|t| &t.task == target)
            || read.not_run.iter().any(|n| &n.task == target);
        let skipped = actions
            .iter()
            .rfind(|a| !a.persistent && a.target == target)
            .is_some_and(|a| a.status == Some("skipped"));
        let Some(of) = deps.get(target).filter(|_| skipped && !judged) else {
            continue;
        };
        let theirs: Vec<&Action> = of
            .iter()
            .filter_map(|d| actions.iter().rfind(|a| a.target == d))
            .collect();
        let failed: Vec<&&Action> = theirs.iter().filter(|a| a.failed(not_run)).collect();
        if !failed.is_empty() {
            for a in &failed {
                let Some(hash) = &a.hash else {
                    continue;
                };
                let listed = targets.iter().any(|t| t == a.target)
                    || read.dependencies.iter().any(|d| d.task == a.target);
                if !listed {
                    read.dependencies
                        .push(a.result(hash.clone(), Outcome::Fail, false, host));
                }
            }
            read.failed_by.push(FailedBy {
                task: target.clone(),
                by: failed.iter().map(|a| a.target.to_string()).collect(),
            });
            continue;
        }
        if let Some((a, code)) = theirs
            .iter()
            .find_map(|a| a.could_not_run(not_run).map(|c| (a, c)))
        {
            read.not_run.push(NotRun {
                task: target.clone(),
                reason: format!(
                    "its dependency {} could not run: {}",
                    a.target,
                    not_run_reason(code)
                ),
                key: None,
            });
        }
    }
    // In target order, whatever order moon ran them in.
    read.tasks.sort_by(|a, b| a.task.cmp(&b.task));
    read.not_run.sort_by(|a, b| a.task.cmp(&b.task));
    read.dependencies.sort_by(|a, b| a.task.cmp(&b.task));
    read.failed_by.sort_by(|a, b| a.task.cmp(&b.task));
    read
}

/// Proposed 119: what moon's run report says of the whole run, `None`
/// when it says nothing of it.
fn run_detail(report: &Value) -> Option<RunDetail> {
    let context = &report["context"];
    // moon 2 names them `changedFiles`; moon 1 named them `touchedFiles`.
    let changed = context["changedFiles"]
        .as_array()
        .or(context["touchedFiles"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(String::from))
        .collect();
    let d = RunDetail {
        took_ms: report_ms(&report["duration"]),
        estimate_ms: report_ms(&report["comparisonEstimate"]["duration"]),
        changed,
    };
    (!d.is_empty()).then_some(d)
}

/// A duration as moon's report writes it, `{"secs": 2, "nanos": 3000000}`,
/// in milliseconds.
fn report_ms(v: &Value) -> Option<u64> {
    let secs = v["secs"].as_u64()?;
    let nanos = v["nanos"].as_u64().unwrap_or(0);
    Some(secs * 1000 + nanos / 1_000_000)
}

/// A task's run record as moon writes it when the task ends.
struct LastRun {
    exit: i32,
    /// `lastRunTime`, milliseconds since the epoch.
    at: i64,
}

fn last_run(place: &Path, target: &str) -> Option<LastRun> {
    let path = state_dir(place, target)?.join("lastRun.json");
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(LastRun {
        exit: i32::try_from(v.get("exitCode")?.as_i64()?).ok()?,
        at: v.get("lastRunTime")?.as_i64()?,
    })
}

/// Where moon keeps a task's run record and logs in a place:
/// `.moon/cache/states/<project>/<task>/`, each id made a file name as
/// moon 2.6's `encode_component` makes it: `@` and `*` dropped, `/` and
/// `:` made `-`, and `-` and `.` trimmed from either end. So
/// `cdk/archetypes:test` keeps its in `cdk-archetypes/test/`. `None` for
/// what is no target.
fn state_dir(place: &Path, target: &str) -> Option<PathBuf> {
    let encode = |id: &str| {
        let mut out = String::with_capacity(id.len());
        for c in id.chars() {
            match c {
                '@' | '*' => {}
                '/' | ':' => out.push('-'),
                c => out.push(c),
            }
        }
        out.trim_matches(['-', '.']).to_string()
    };
    let (project, task) = target.split_once(':')?;
    Some(
        place
            .join(".moon/cache/states")
            .join(encode(project))
            .join(encode(task)),
    )
}

/// What moon's checkpoints said of one task: `▮▮▮▮ a:test (f8bf095b)` as
/// it starts, `▮▮▮▮ a:test (1s 65ms, f8bf095b)` as it ends, and
/// `▮▮▮▮ b:test (cached, b7d5ce19)` for a task answered from the cache,
/// which has no start line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mark {
    cached: bool,
    /// How long it took, in milliseconds, once it ended.
    took: Option<i64>,
    /// Where its start line ends in the output, through its newline.
    start: Option<u64>,
}

/// Each task moon's checkpoints name in a run's output. A task's own
/// output is prefixed with its target, so no line of it is a checkpoint.
fn checkpoints(output: &[u8]) -> BTreeMap<String, Mark> {
    let mut out: BTreeMap<String, Mark> = BTreeMap::new();
    let mut end = 0;
    for line in output.split(|b| *b == b'\n') {
        end = (end + line.len() + 1).min(output.len());
        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        if !line.contains('\u{25AE}') {
            continue;
        }
        let plain = plain(line);
        let Some(rest) = plain.strip_prefix(CHECKPOINT) else {
            continue;
        };
        let rest = rest.trim_end();
        let Some((target, inner)) = rest.split_once(" (") else {
            continue;
        };
        let Some(inner) = inner.strip_suffix(')') else {
            continue;
        };
        let parts: Vec<&str> = inner.split(", ").collect();
        let mark = out.entry(target.to_string()).or_default();
        if parts.contains(&"cached") {
            mark.cached = true;
        }
        if let Some(took) = parts.iter().find_map(|p| duration_ms(p)) {
            mark.took = Some(took);
        } else if !mark.cached && mark.start.is_none() {
            mark.start = Some(end as u64);
        }
    }
    out
}

/// A line with its colour codes taken out: an escape, `[`, and
/// everything up to its final letter.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// One of moon's durations, `2m 3s`, `1s 65ms` or `1ms`, in milliseconds.
fn duration_ms(text: &str) -> Option<i64> {
    let mut total = 0.0;
    for word in text.split(' ') {
        let at = word.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let n: f64 = word[..at].parse().ok()?;
        total += n * match &word[at..] {
            "ms" => 1.0,
            "s" => 1_000.0,
            "m" => 60_000.0,
            "h" => 3_600_000.0,
            _ => return None,
        };
    }
    Some(total.round() as i64)
}

fn timestamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Design D2 of the change `a-pass-nothing-proves-blames-no-one` (C.6;
/// record 0105): a manifest a key needs is missing from the store, the
/// key's own or a dependency's at any depth, so the run kept no proof of
/// the key. `task` is the task whose manifest is missing, `key` its key.
/// Every other failure of [`check_key`] is a key the tree does not bear
/// out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoManifest {
    pub task: String,
    pub key: String,
}

impl std::fmt::Display for NoManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: no manifest for {}", self.task, self.key)
    }
}

impl std::error::Error for NoManifest {}

/// The marks moon 2.6.1's `dep_hash_input` gives a dependency in a hash
/// manifest's `deps` in place of a key: `ignored`, its default for a
/// dependency with no outputs; `passthrough`, for an edge the graph
/// ignores or a dependency that passed through; and `outputs` and
/// `outputs-passthrough`, for one whose outputs are among the task's
/// inputs, which the manifest's inputs then name (C.2 moon row, C.6).
pub const DEPENDENCY_MARKS: [&str; 4] =
    ["ignored", "passthrough", "outputs", "outputs-passthrough"];

/// Design D9 (C.6): whether `key` is moon's key for `task` on the
/// combination's tree, checked without running anything. moon keeps each
/// task's hash manifest in its blob store under its key, the key the
/// sha256 of the manifest; the manifest names the task, every input with
/// the sha256 of its content, and each dependency's key or one of the
/// four marks moon 2.6.1's `dep_hash_input` writes for a dependency that
/// gives the task no key ([`DEPENDENCY_MARKS`]), which names no manifest
/// and is walked no further (design D2 of the change
/// `a-signed-run-without-a-sandbox-proves-its-passes`). Any other value
/// that is not a key is refused. `blob` reads the store, `tree` a path of
/// the combination's tree. A signer signs only keys this passes. Where a
/// manifest the walk needs is missing, it fails with [`NoManifest`].
pub fn check_key(
    key: &str,
    task: &str,
    blob: &dyn Fn(&str) -> Option<Vec<u8>>,
    tree: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<()> {
    fn go(
        key: &str,
        task: &str,
        blob: &dyn Fn(&str) -> Option<Vec<u8>>,
        tree: &dyn Fn(&str) -> Option<Vec<u8>>,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            bail!("{task}: dependencies deeper than any graph");
        }
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("{task}: {key:?} is no key");
        }
        let bytes = blob(key).ok_or_else(|| NoManifest {
            task: task.to_string(),
            key: key.to_string(),
        })?;
        if sha256_hex(&bytes) != key {
            bail!("{task}: the manifest under {key} does not hash to it");
        }
        let manifest: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("{task}: the manifest under {key}"))?;
        let m = &manifest[0];
        if m["target"].as_str() != Some(task) {
            bail!("{task}: the manifest under {key} is {}'s", m["target"]);
        }
        let inputs = m["inputs"]
            .as_object()
            .ok_or_else(|| anyhow!("{task}: the manifest names no inputs"))?;
        for (path, sum) in inputs {
            let held = tree(path).ok_or_else(|| anyhow!("{task}: {path} is not in the tree"))?;
            if Some(sha256_hex(&held).as_str()) != sum.as_str() {
                bail!("{task}: {path} is not what the tree holds");
            }
        }
        if let Some(deps) = m["deps"].as_object() {
            for (dep, k) in deps {
                match k.as_str() {
                    Some(mark) if DEPENDENCY_MARKS.contains(&mark) => {}
                    Some(k) => go(k, dep, blob, tree, depth + 1)?,
                    None => bail!("{task}: dependency {dep} has no key"),
                }
            }
        }
        Ok(())
    }
    go(key, task, blob, tree, 0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// moon's blob store under `store`: `<2>/<62>` of a key.
pub fn blob_in(store: &Path, key: &str) -> Option<Vec<u8>> {
    if key.len() < 3 || !key.is_ascii() {
        return None;
    }
    std::fs::read(store.join("blobs").join(&key[..2]).join(&key[2..])).ok()
}

/// Design D3 of the change `a-signed-run-without-a-sandbox-proves-its-
/// passes` (114; record 0105): what a store a manifest is missing from
/// says of itself, read from the store after the run with nothing run,
/// never from what moon printed (rule core.35). The first that holds is
/// the one given, ordered so it names first what the host must change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreFact {
    /// The store is not a directory.
    Absent,
    /// The user sluice runs as cannot write the store's `blobs`, or the
    /// store where it holds no `blobs`.
    Unwritable { user: String },
    /// The user sluice runs as cannot write the key's shard of `blobs`.
    ShardUnwritable { user: String },
    /// No key of the run's tasks and dependencies has a blob in either
    /// store the run's moon could keep it in.
    NoneKept,
    /// Some key of the run's has a blob there.
    OthersKept,
}

impl std::fmt::Display for StoreFact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreFact::Absent => f.write_str("which does not exist"),
            StoreFact::Unwritable { user } => write!(f, "which {user} cannot write"),
            StoreFact::ShardUnwritable { user } => {
                write!(f, "a directory of which {user} cannot write")
            }
            StoreFact::NoneKept => f.write_str("where moon kept none of the run's manifests"),
            StoreFact::OthersKept => f.write_str("where moon kept others of the run's manifests"),
        }
    }
}

/// [`StoreFact`] of `store`, for the manifest under `key` it lacks, where
/// `kept` of the keys of the run's tasks and dependencies have a blob in
/// either store the run's moon could keep it in. Whether the user sluice
/// runs as can write a directory is `access(2)`'s answer for this
/// process, the open executor's run being its child under its user.
pub fn store_fact(store: &Path, key: &str, kept: usize) -> StoreFact {
    use nix::unistd::{access, AccessFlags};
    if !store.is_dir() {
        return StoreFact::Absent;
    }
    let writes = |dir: &Path| access(dir, AccessFlags::W_OK).is_ok();
    let blobs = store.join("blobs");
    let top = if blobs.is_dir() { &blobs } else { store };
    if !writes(top) {
        return StoreFact::Unwritable { user: user() };
    }
    if key.is_ascii() && key.len() >= 2 {
        let shard = blobs.join(&key[..2]);
        if shard.is_dir() && !writes(&shard) {
            return StoreFact::ShardUnwritable { user: user() };
        }
    }
    if kept == 0 {
        StoreFact::NoneKept
    } else {
        StoreFact::OthersKept
    }
}

/// The name of the user sluice runs as, as `access(2)` reads it, else its
/// uid.
fn user() -> String {
    let uid = nix::unistd::getuid();
    match nix::unistd::User::from_uid(uid) {
        Ok(Some(u)) => u.name,
        _ => uid.to_string(),
    }
}

/// Design D8 (C.2 moon row, 35): where moon keeps the blobs and hash
/// manifests of a run in `checkout` whose command starts in `env`, found as
/// moon 2.6.1 finds it (`crates/app/src/session.rs`, `get_cache_engine`),
/// and never from where the host's `[[caches]]` says. `env` is the
/// environment as the command sees it, `[tests] toolchain` sourced
/// ([`super::checks::Context::command_env`]): a toolchain that sets
/// `MOON_HOME` moves moon's home, as sluice's own devenv does. With moon's shared worktree
/// setting on in a linked worktree, it is the repository's base checkout's
/// `.moon/cache` when the base holds `.moon`, else `cache/shared` under
/// moon's home; otherwise, the base checkout itself among it, the
/// checkout's own `.moon/cache`.
pub fn store(checkout: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    // moon's working directory is the checkout, resolved.
    let checkout = checkout
        .canonicalize()
        .unwrap_or_else(|_| checkout.to_path_buf());
    let own = checkout.join(".moon/cache");
    if !shares_worktree_cache(&checkout, env) {
        return own;
    }
    let Ok(repo) = gix::discover(&checkout) else {
        return own;
    };
    if !matches!(repo.kind(), gix::repository::Kind::LinkedWorkTree) {
        return own;
    }
    // `get_repository_root`: the common directory, or its parent when it
    // is a `.git`.
    let mut root = lexical(repo.common_dir());
    if root.ends_with(".git") {
        root.pop();
    }
    let work = lexical(repo.workdir().unwrap_or(repo.git_dir()));
    if root.join(".moon").exists() && root != work {
        root.join(".moon/cache")
    } else {
        moon_home(&checkout, env).join("cache/shared")
    }
}

/// moon's shared worktree setting for a run given `env` (moon 2.6.1,
/// `crates/config/src/workspace/cache_config.rs`). moon applies the
/// environment over its files, so `MOON_CACHE_SHARED_WORKTREE_CACHE`, when
/// set, decides alone: on for `1`, `true`, `yes`, `on`, `enabled` or
/// `enable` in any case, off for anything else, empty included. Unset, the
/// workspace's `cache.unstable_sharedWorktreeCache` decides, off when it
/// says nothing.
fn shares_worktree_cache(checkout: &Path, env: &BTreeMap<String, String>) -> bool {
    if let Some(v) = env.get("MOON_CACHE_SHARED_WORKTREE_CACHE") {
        return matches!(
            v.to_lowercase().as_str(),
            "1" | "true" | "yes" | "on" | "enabled" | "enable"
        );
    }
    // moon reads both files, the later over the earlier.
    ["workspace.yml", "workspace.yaml"]
        .iter()
        .rev()
        .filter_map(|f| std::fs::read_to_string(checkout.join(".moon").join(f)).ok())
        .find_map(|yaml| workspace_setting(&yaml))
        .unwrap_or(false)
}

/// `cache.unstable_sharedWorktreeCache` of a workspace's YAML, as a
/// mapping under `cache` in block or flow style, the boolean read as
/// moon's YAML reads one; `None` when the workspace does not say.
fn workspace_setting(yaml: &str) -> Option<bool> {
    let key = regex::Regex::new(r"unstable_sharedWorktreeCache\s*:\s*([A-Za-z]+)").unwrap();
    let boolean = |text: &str| {
        key.captures(text).and_then(|c| match &c[1] {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        })
    };
    let mut lines = yaml.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix("cache:") else {
            continue;
        };
        if rest.trim_start().starts_with('{') {
            return boolean(rest);
        }
        // The block ends at the next line that is neither indented, blank
        // nor a comment.
        return lines
            .take_while(|l| l.starts_with([' ', '\t']) || l.trim().is_empty() || l.starts_with('#'))
            .filter(|l| !l.trim_start().starts_with('#'))
            .find_map(boolean);
    }
    None
}

/// moon's home, as moon 2.6.1 reads it from a run's `env`
/// (`crates/env/src/lib.rs`): `MOON_HOME`, else `$XDG_DATA_HOME/moon`,
/// else `.moon` under `HOME` or the user's home directory; a relative
/// path is taken from the run's working directory, `cwd`, and an empty
/// variable is unset.
fn moon_home(cwd: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    let set = |k: &str| env.get(k).filter(|v| !v.is_empty());
    let path = |k: &str| set(k).map(|v| cwd.join(v));
    path("MOON_HOME")
        .or_else(|| path("XDG_DATA_HOME").map(|x| x.join("moon")))
        .unwrap_or_else(|| {
            set("HOME")
                .map(PathBuf::from)
                .or_else(std::env::home_dir)
                .unwrap_or_default()
                .join(".moon")
        })
}

/// `path` with `.` and `..` taken out by its components alone, as moon's
/// `clean_components` does: a linked worktree's common directory is given
/// as `<git dir>/../..`.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR 0050: a task kept out of CI is named with the entry that names
    /// it, its own name before a tag; one in CI, or not required, is not.
    #[test]
    fn a_required_task_kept_out_of_ci_is_named_with_its_entry() {
        let task = |tags: &[&str], in_ci| Task {
            tags: tags.iter().map(|t| t.to_string()).collect(),
            in_ci,
            deps: Vec::new(),
        };
        let tasks = BTreeMap::from([
            ("app:serve".to_string(), task(&["check-fast"], false)),
            ("app:test".to_string(), task(&["check-fast"], true)),
            ("root:integration".to_string(), task(&["check-fast"], false)),
            ("root:screenshots".to_string(), task(&[], false)),
        ]);
        let required = ["#check-fast".to_string(), "root:integration".to_string()];
        let kept: Vec<(String, String)> = kept_out(&tasks, &required)
            .into_iter()
            .map(|k| (k.task, k.entry))
            .collect();
        assert_eq!(
            kept,
            [
                ("app:serve".to_string(), "#check-fast".to_string()),
                (
                    "root:integration".to_string(),
                    "root:integration".to_string()
                ),
            ]
        );
    }

    /// 35: a run is cold when it holds a test `[tests] cold` names, by
    /// its name or a tag it carries; tests it does not hold make it warm.
    #[test]
    fn a_run_holding_a_cold_test_is_cold() {
        let task = |tags: &[&str]| Task {
            tags: tags.iter().map(|t| t.to_string()).collect(),
            in_ci: true,
            deps: Vec::new(),
        };
        let tasks = BTreeMap::from([
            ("app:bench".to_string(), task(&["check-fast", "bench"])),
            ("app:test".to_string(), task(&["check-fast"])),
            ("root:timing".to_string(), task(&[])),
        ]);
        let s = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let bench = s(&["#bench"]);
        assert!(holds_cold(&tasks, &s(&["app:bench", "app:test"]), &bench));
        assert!(!holds_cold(&tasks, &s(&["app:test"]), &bench));
        assert!(holds_cold(
            &tasks,
            &s(&["root:timing"]),
            &s(&["root:timing"])
        ));
        assert!(!holds_cold(&tasks, &s(&["app:test"]), &s(&["root:timing"])));
        assert!(!holds_cold(&tasks, &s(&["app:bench"]), &[]));
        // A tag names no task by its own name.
        assert!(!holds_cold(&tasks, &s(&["app:test"]), &s(&["#app:test"])));
    }

    /// 35: `[tests] cold` is a list of tests; anything else is refused.
    #[test]
    fn a_cold_that_is_no_list_is_refused() {
        use sluice_seams::{Setting, Settings};
        assert_eq!(cold_tests(&Settings::new()).unwrap(), Vec::<String>::new());
        let list = Settings::from([(
            "cold".to_string(),
            Setting::List(vec!["#bench".to_string()]),
        )]);
        assert_eq!(cold_tests(&list).unwrap(), ["#bench"]);
        let text = Settings::from([("cold".to_string(), Setting::Text("#bench".into()))]);
        assert_eq!(
            cold_tests(&text).unwrap_err().to_string(),
            "[tests] cold must be a list of tests, named as required names them"
        );
    }

    mod keys {
        use super::super::*;
        use std::collections::BTreeMap;

        /// A blob store and a tree, built as moon builds them.
        struct Store(BTreeMap<String, Vec<u8>>);

        impl Store {
            fn manifest(
                &mut self,
                task: &str,
                inputs: &[(&str, &str)],
                deps: &[(&str, &str)],
            ) -> String {
                let inputs: BTreeMap<&str, String> = inputs
                    .iter()
                    .map(|(p, c)| (*p, sha256_hex(c.as_bytes())))
                    .collect();
                let deps: BTreeMap<&str, &str> = deps.iter().copied().collect();
                let bytes = serde_json::to_vec(&serde_json::json!([{
                    "command": "true", "args": [], "deps": deps, "env": {},
                    "inputs": inputs, "inputEnv": {}, "outputs": [], "projectDeps": [],
                    "target": task, "toolchains": ["system"], "version": "3"
                }]))
                .unwrap();
                let key = sha256_hex(&bytes);
                self.0.insert(key.clone(), bytes);
                key
            }
        }

        fn tree(files: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<Vec<u8>> {
            let files: BTreeMap<&str, &str> = files.iter().copied().collect();
            move |p: &str| files.get(p).map(|c| c.as_bytes().to_vec())
        }

        #[test]
        fn a_key_the_tree_bears_out_passes() {
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "1")], &[]);
            let b = s.manifest(
                "b:test",
                &[("b/y", "2")],
                &[("a:test", &a), ("c:lint", "ignored")],
            );
            let blob = |k: &str| s.0.get(k).cloned();
            let t = tree(&[("a/x", "1"), ("b/y", "2")]);
            check_key(&b, "b:test", &blob, &t).unwrap();
        }

        #[test]
        fn a_key_of_another_combination_is_refused() {
            let mut s = Store(BTreeMap::new());
            let other = s.manifest("a:test", &[("a/x", "other")], &[]);
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&other, "a:test", &blob, &tree(&[("a/x", "1")])).unwrap_err();
            assert!(
                e.to_string().contains("a/x is not what the tree holds"),
                "{e}"
            );
        }

        #[test]
        fn an_altered_manifest_is_refused() {
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "1")], &[]);
            let mut forged = s.0[&a].clone();
            forged.extend(b" ");
            s.0.insert(a.clone(), forged);
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&a, "a:test", &blob, &tree(&[("a/x", "1")])).unwrap_err();
            assert!(e.to_string().contains("does not hash"), "{e}");
        }

        #[test]
        fn a_key_named_for_another_task_is_refused() {
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "1")], &[]);
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&a, "b:test", &blob, &tree(&[("a/x", "1")])).unwrap_err();
            assert!(e.to_string().contains("a:test"), "{e}");
            let none = check_key(&"0".repeat(64), "a:test", &blob, &tree(&[])).unwrap_err();
            assert!(none.to_string().contains("no manifest"), "{none}");
        }

        #[test]
        fn a_dependency_the_tree_does_not_bear_out_is_refused() {
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "stale")], &[]);
            let b = s.manifest("b:test", &[("b/y", "2")], &[("a:test", &a)]);
            let blob = |k: &str| s.0.get(k).cloned();
            let e =
                check_key(&b, "b:test", &blob, &tree(&[("a/x", "1"), ("b/y", "2")])).unwrap_err();
            assert!(e.to_string().starts_with("a:test:"), "{e}");
        }

        /// Design D2 of `a-signed-run-without-a-sandbox-proves-its-passes`:
        /// a dependency moon 2.6.1 names by one of its four marks gives no
        /// key and is walked no further, so the key is borne out walking
        /// its keyed dependency alone; any other value that is no key is
        /// refused as one.
        #[test]
        fn moons_dependency_marks_are_no_key() {
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "1")], &[]);
            let b = s.manifest(
                "b:test",
                &[("b/y", "2")],
                &[
                    ("a:test", &a),
                    ("c:lint", "ignored"),
                    ("d:gen", "passthrough"),
                    ("e:build", "outputs"),
                    ("f:setup", "outputs-passthrough"),
                ],
            );
            let t = tree(&[("a/x", "1"), ("b/y", "2")]);
            let walked = std::cell::RefCell::new(Vec::new());
            let blob = |k: &str| {
                walked.borrow_mut().push(k.to_string());
                s.0.get(k).cloned()
            };
            check_key(&b, "b:test", &blob, &t).unwrap();
            assert_eq!(*walked.borrow(), [b.clone(), a.clone()]);

            let stale = s.manifest("b:test", &[("b/y", "2")], &[("c:lint", "stale")]);
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&stale, "b:test", &blob, &t).unwrap_err();
            assert_eq!(e.downcast_ref::<NoManifest>(), None, "{e}");
            assert_eq!(e.to_string(), "c:lint: \"stale\" is no key");
        }

        /// Design D2: a missing manifest, the key's own or a dependency's,
        /// is a proof the run did not keep, told apart by its type and
        /// naming the task whose manifest is missing; a manifest that does
        /// not hash to its key, or an input the tree does not hold, is a
        /// key the tree does not bear out.
        #[test]
        fn a_missing_manifest_is_told_from_one_the_tree_does_not_bear_out() {
            let missing = |e: &anyhow::Error| e.downcast_ref::<NoManifest>().cloned();
            let mut s = Store(BTreeMap::new());
            let a = s.manifest("a:test", &[("a/x", "1")], &[]);
            let b = s.manifest("b:test", &[("b/y", "2")], &[("a:test", &a)]);
            let c = s.manifest("c:test", &[("c/z", "3")], &[("b:test", &b)]);
            let t = tree(&[("a/x", "1"), ("b/y", "2"), ("c/z", "3")]);

            // The key's own manifest gone.
            let gone = "0".repeat(64);
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&gone, "a:test", &blob, &t).unwrap_err();
            assert_eq!(
                missing(&e),
                Some(NoManifest {
                    task: "a:test".into(),
                    key: gone.clone()
                })
            );
            assert_eq!(e.to_string(), format!("a:test: no manifest for {gone}"));

            // A dependency's, two deep.
            let mut held = s.0.clone();
            held.remove(&a);
            let blob = |k: &str| held.get(k).cloned();
            let e = check_key(&c, "c:test", &blob, &t).unwrap_err();
            assert_eq!(
                missing(&e),
                Some(NoManifest {
                    task: "a:test".into(),
                    key: a.clone()
                }),
                "{e}"
            );

            // An altered manifest.
            let mut forged = s.0.clone();
            forged.get_mut(&a).unwrap().extend(b" ");
            let blob = |k: &str| forged.get(k).cloned();
            let e = check_key(&b, "b:test", &blob, &t).unwrap_err();
            assert_eq!(missing(&e), None, "{e}");

            // An input the tree does not hold.
            let blob = |k: &str| s.0.get(k).cloned();
            let e = check_key(&b, "b:test", &blob, &tree(&[("b/y", "2")])).unwrap_err();
            assert!(e.to_string().contains("a/x is not in the tree"), "{e}");
            assert_eq!(missing(&e), None, "{e}");
        }
    }

    /// Design D8: where moon 2.6.1 keeps a run's manifests, over
    /// repositories laid out as git lays them out.
    mod stores {
        use super::super::*;
        use crate::instance::HostCache;

        /// A directory, resolved as moon's working directory is, removed
        /// when the test ends.
        struct Scratch(PathBuf);

        impl Scratch {
            fn new(name: &str) -> Scratch {
                let dir = std::env::temp_dir()
                    .join(format!("sluice-moon-store-{}-{name}", std::process::id()));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                Scratch(dir.canonicalize().unwrap())
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        /// A linked worktree at `at` of the repository whose common
        /// directory is `common`, as `git worktree add` writes one.
        fn worktree(common: &Path, at: &Path) {
            let admin = common.join("worktrees").join(at.file_name().unwrap());
            std::fs::create_dir_all(&admin).unwrap();
            std::fs::create_dir_all(at).unwrap();
            std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
            std::fs::write(admin.join("commondir"), "../..\n").unwrap();
            std::fs::write(
                admin.join("gitdir"),
                format!("{}\n", at.join(".git").display()),
            )
            .unwrap();
            std::fs::write(at.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        }

        fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        }

        const ON: (&str, &str) = ("MOON_CACHE_SHARED_WORKTREE_CACHE", "true");

        /// Design D3 of `a-signed-run-without-a-sandbox-proves-its-passes`:
        /// a store a manifest is missing from says, in turn, that it does
        /// not exist, that its user cannot write it or the key's directory
        /// of it, and whether moon kept the run's other manifests there,
        /// each case in a directory of its own. `access(2)` refuses root
        /// nothing, so as root the two unwritable cases are skipped.
        #[test]
        fn the_store_says_why_a_manifest_is_missing() {
            use std::os::unix::fs::PermissionsExt;
            let s = Scratch::new("facts");
            let key = format!("ab{}", "0".repeat(62));
            let mode = |p: &Path, m: u32| {
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap()
            };
            let me = user();

            let absent = s.0.join("absent");
            assert_eq!(store_fact(&absent, &key, 3), StoreFact::Absent);
            assert_eq!(
                store_fact(&absent, &key, 3).to_string(),
                "which does not exist"
            );
            let file = s.0.join("a-file");
            std::fs::write(&file, "").unwrap();
            assert_eq!(store_fact(&file, &key, 3), StoreFact::Absent);

            let none = s.0.join("none");
            std::fs::create_dir_all(none.join("blobs")).unwrap();
            assert_eq!(store_fact(&none, &key, 0), StoreFact::NoneKept);
            assert_eq!(
                store_fact(&none, &key, 0).to_string(),
                "where moon kept none of the run's manifests"
            );

            let others = s.0.join("others");
            std::fs::create_dir_all(others.join("blobs/cd")).unwrap();
            assert_eq!(store_fact(&others, &key, 2), StoreFact::OthersKept);
            assert_eq!(
                store_fact(&others, &key, 2).to_string(),
                "where moon kept others of the run's manifests"
            );

            if nix::unistd::getuid().is_root() {
                eprintln!(
                    "skipped: as root, access(2) refuses no write, so a store \
                     its user cannot write is not made here"
                );
                return;
            }

            // `blobs` its user cannot write.
            let locked = s.0.join("locked");
            std::fs::create_dir_all(locked.join("blobs")).unwrap();
            mode(&locked.join("blobs"), 0o555);
            let fact = store_fact(&locked, &key, 2);
            mode(&locked.join("blobs"), 0o755);
            assert_eq!(fact, StoreFact::Unwritable { user: me.clone() });
            assert_eq!(fact.to_string(), format!("which {me} cannot write"));

            // No `blobs`, and the store its user cannot write.
            let bare = s.0.join("bare");
            std::fs::create_dir_all(&bare).unwrap();
            mode(&bare, 0o555);
            let fact = store_fact(&bare, &key, 0);
            mode(&bare, 0o755);
            assert_eq!(fact, StoreFact::Unwritable { user: me.clone() });

            // The key's directory its user cannot write.
            let shard = s.0.join("shard");
            std::fs::create_dir_all(shard.join("blobs/ab")).unwrap();
            mode(&shard.join("blobs/ab"), 0o555);
            let fact = store_fact(&shard, &key, 2);
            let other = store_fact(&shard, &format!("cd{}", "0".repeat(62)), 2);
            mode(&shard.join("blobs/ab"), 0o755);
            assert_eq!(fact, StoreFact::ShardUnwritable { user: me.clone() });
            assert_eq!(
                fact.to_string(),
                format!("a directory of which {me} cannot write")
            );
            assert_eq!(other, StoreFact::OthersKept);
        }

        #[test]
        fn a_key_is_read_where_moon_keeps_it() {
            let s = Scratch::new("keeps");
            let home = s.0.join("home");

            // A repository whose base checkout holds `.moon`, a worktree
            // beside it.
            let base = s.0.join("base");
            gix::init(&base).unwrap();
            std::fs::create_dir_all(base.join(".moon")).unwrap();
            let linked = s.0.join("linked");
            worktree(&base.join(".git"), &linked);
            std::fs::create_dir_all(linked.join(".moon")).unwrap();
            let shared = base.join(".moon/cache");

            // The setting off: the checkout's own.
            let none = env(&[("HOME", home.to_str().unwrap())]);
            assert_eq!(store(&linked, &none), linked.join(".moon/cache"));
            // On, in a linked worktree: the base's, where moon wrote the
            // manifest.
            let on = env(&[ON, ("HOME", home.to_str().unwrap())]);
            assert_eq!(store(&linked, &on), shared);
            let key = "ab".to_string() + &"c".repeat(62);
            let blobs = shared.join("blobs/ab");
            std::fs::create_dir_all(&blobs).unwrap();
            std::fs::write(blobs.join("c".repeat(62)), b"manifest").unwrap();
            assert_eq!(
                blob_in(&store(&linked, &on), &key).as_deref(),
                Some(&b"manifest"[..])
            );
            assert_eq!(blob_in(&store(&linked, &none), &key), None);
            // On, in the base checkout itself: its own.
            assert_eq!(store(&base, &on), shared);
            assert_eq!(store(&base, &none), shared);

            // A worktree of a bare repository: moon's home, from the
            // run's environment.
            let bare = s.0.join("repo.git");
            gix::init_bare(&bare).unwrap();
            let main = s.0.join("main");
            worktree(&bare, &main);
            let moon_home = s.0.join("moon-home");
            let with_home = env(&[
                ON,
                ("HOME", home.to_str().unwrap()),
                ("MOON_HOME", moon_home.to_str().unwrap()),
            ]);
            assert_eq!(store(&main, &with_home), moon_home.join("cache/shared"));
            assert_eq!(store(&main, &on), home.join(".moon/cache/shared"));
            let xdg = env(&[
                ON,
                ("HOME", home.to_str().unwrap()),
                ("XDG_DATA_HOME", s.0.join("data").to_str().unwrap()),
            ]);
            assert_eq!(store(&main, &xdg), s.0.join("data/moon/cache/shared"));
            assert_eq!(store(&main, &none), main.join(".moon/cache"));

            // A host cache whose `dir` is elsewhere moves nothing.
            let elsewhere = HostCache {
                dir: s.0.join("caches/moon"),
                socket: None,
                env: env(&[ON]),
            };
            let mut declared = env(&[("HOME", home.to_str().unwrap())]);
            declared.extend(elsewhere.env.clone());
            assert_eq!(store(&linked, &declared), shared);
            assert_eq!(store(&main, &declared), home.join(".moon/cache/shared"));
        }

        /// The variable, when set, decides over the workspace's file, as
        /// moon applies the environment last; unset, the file decides.
        #[test]
        fn the_variable_decides_over_the_workspace() {
            let s = Scratch::new("decides");
            let base = s.0.join("base");
            gix::init(&base).unwrap();
            std::fs::create_dir_all(base.join(".moon")).unwrap();
            let linked = s.0.join("linked");
            worktree(&base.join(".git"), &linked);
            std::fs::create_dir_all(linked.join(".moon")).unwrap();
            let own = linked.join(".moon/cache");
            let shared = base.join(".moon/cache");
            let var = |v: &str| env(&[("MOON_CACHE_SHARED_WORKTREE_CACHE", v)]);

            for on in ["1", "true", "TRUE", "yes", "On", "enabled", "enable"] {
                assert_eq!(store(&linked, &var(on)), shared, "{on}");
            }
            for off in ["0", "false", "", "no", "y"] {
                assert_eq!(store(&linked, &var(off)), own, "{off:?}");
            }

            let workspace = linked.join(".moon/workspace.yml");
            std::fs::write(
                &workspace,
                "projects: {}\ncache:\n  # shared among worktrees\n  cas:\n    verifyIntegrity: true\n  unstable_sharedWorktreeCache: true\nvcs:\n  client: git\n",
            )
            .unwrap();
            assert_eq!(store(&linked, &BTreeMap::new()), shared);
            assert_eq!(store(&linked, &var("false")), own);
            std::fs::write(
                &workspace,
                "cache: { unstable_sharedWorktreeCache: True }\n",
            )
            .unwrap();
            assert_eq!(store(&linked, &BTreeMap::new()), shared);
            std::fs::write(
                &workspace,
                "cache:\n  unstable_sharedWorktreeCache: false\nunstable_sharedWorktreeCache: true\n",
            )
            .unwrap();
            assert_eq!(store(&linked, &BTreeMap::new()), own);
            assert_eq!(store(&linked, &var("yes")), shared);
        }
    }

    /// Shapes captured from moon's run reports (section 3).
    #[test]
    fn reads_runs_replays_failures_and_helpers() {
        let report: Value = serde_json::from_str(
            r#"{"actions":[
              {"label":"SyncWorkspace","status":"passed"},
              {"label":"RunTask(a:test)","status":"passed","node":{"params":{"target":"a:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h1"}},
                             {"status":"skipped","meta":{"type":"output-hydration"}},
                             {"status":"passed","meta":{"type":"task-execution","exitCode":0}}]},
              {"label":"RunTask(b:test)","status":"cached","node":{"params":{"target":"b:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h2"}},
                             {"status":"cached","meta":{"type":"output-hydration","exitCode":0}}]},
              {"label":"RunTask(c:test)","status":"failed","allowFailure":true,"node":{"params":{"target":"c:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h3"}},
                             {"status":"failed","meta":{"type":"task-execution","exitCode":1}}]},
              {"label":"RunTask(d:test)","status":"skipped","node":{"params":{"target":"d:test"}},"operations":[]},
              {"label":"RunPersistentTask(e:serve)","status":"passed","node":{"params":{"target":"e:serve"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h5"}}]},
              {"label":"RunTask(z:other)","status":"passed","node":{"params":{"target":"z:other"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h6"}}]}
            ]}"#,
        )
        .unwrap();
        let targets: Vec<String> = ["a:test", "b:test", "c:test", "d:test", "e:serve"]
            .map(String::from)
            .to_vec();
        let Read {
            tasks: got,
            not_run: unjudged,
            ..
        } = read_report(&report, &targets, &BTreeMap::new(), &[], "h");
        assert!(unjudged.is_empty());
        let short: Vec<(&str, &str, Outcome, bool)> = got
            .iter()
            .map(|t| {
                (
                    t.task.as_str(),
                    t.hash.as_str(),
                    t.outcome,
                    matches!(t.source, Source::Replayed { .. }),
                )
            })
            .collect();
        assert_eq!(
            short,
            vec![
                ("a:test", "h1", Outcome::Pass, false),
                ("b:test", "h2", Outcome::Pass, true),
                ("c:test", "h3", Outcome::Fail, false),
            ]
        );
        // Could-not-run-1 114: an exit `not_run` names is no fail.
        let Read {
            tasks: got,
            not_run: unjudged,
            ..
        } = read_report(&report, &targets, &BTreeMap::new(), &[1], "h");
        assert!(!got.iter().any(|t| t.task == "c:test"));
        assert_eq!(unjudged.len(), 1);
        assert_eq!(unjudged[0].task, "c:test");
        assert!(
            unjudged[0].reason.contains("exited 1"),
            "{}",
            unjudged[0].reason
        );
    }

    /// A run report as moon 2.6.1 wrote it for `moon exec app:test
    /// app:retry app:serve --ci`, trimmed: `app:retry` failed its first try
    /// and passed its second, and `app:serve`, marked `runInCI: false`, is
    /// absent (119; ADR 0044).
    #[test]
    fn reads_times_attempts_and_the_runs_estimate() {
        let report: Value = serde_json::from_str(
            r#"{"actions":[
              {"label":"RunTask(app:test)","status":"passed","duration":{"secs":0,"nanos":175123708},
               "node":{"params":{"target":"app:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"88c9720a"}},
                             {"status":"skipped","meta":{"type":"output-hydration"}},
                             {"status":"passed","meta":{"type":"task-execution","exitCode":0}},
                             {"status":"passed","meta":{"type":"archive-creation"}}]},
              {"label":"RunTask(app:retry)","status":"passed","duration":{"secs":0,"nanos":333944708},
               "node":{"params":{"target":"app:retry"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"5cfc0d6e"}},
                             {"status":"failed","meta":{"type":"task-execution","exitCode":1}},
                             {"status":"passed","meta":{"type":"task-execution","exitCode":0}}]}],
             "context":{"changedFiles":["app/f"]},
             "duration":{"secs":0,"nanos":341842792},
             "comparisonEstimate":{"duration":{"secs":0,"nanos":431000000}},
             "status":"completed"}"#,
        )
        .unwrap();
        let targets: Vec<String> = ["app:retry", "app:serve", "app:test"]
            .map(String::from)
            .to_vec();
        // An exit `not_run` names on a first try that a retry passed is no
        // could-not-run.
        let Read {
            tasks: got,
            not_run: unjudged,
            ..
        } = read_report(&report, &targets, &BTreeMap::new(), &[1], "h");
        assert!(unjudged.is_empty());
        let short: Vec<(&str, Outcome, Option<u64>, Option<u32>)> = got
            .iter()
            .map(|t| (t.task.as_str(), t.outcome, t.took_ms, t.attempts))
            .collect();
        assert_eq!(
            short,
            vec![
                ("app:retry", Outcome::Pass, Some(333), Some(2)),
                ("app:test", Outcome::Pass, Some(175), Some(1)),
            ]
        );
        let d = run_detail(&report).unwrap();
        assert_eq!(d.took_ms, Some(341));
        assert_eq!(d.estimate_ms, Some(431));
        assert_eq!(d.saved_ms(), Some(90));
        assert_eq!(d.changed, vec!["app/f".to_string()]);
        assert_eq!(run_detail(&serde_json::json!({"actions": []})), None);
    }

    /// A `moon query tasks` document as moon 2.6.1 prints it, trimmed to
    /// what the runner reads: each dependency an object with `target`,
    /// `args`, `env` and `cacheStrategy`, and `type` for a `wait` or a
    /// `cleanup` one; a task with none has no `deps`. `root:all` also
    /// names `ui:test` as a bare target, and an item of no shape moon
    /// prints (149; C.2).
    const QUERY: &str = r#"{"tasks":{
      "alpha":{
        "serve":{"target":"alpha:serve","tags":[],"options":{"runInCI":true,"persistent":true}},
        "stop":{"target":"alpha:stop","tags":[],"options":{"runInCI":true}}},
      "api":{
        "build":{"target":"api:build","tags":[],"options":{"runInCI":true},
          "deps":[{"args":[],"env":{},"target":"api:test","cacheStrategy":"ignored"}]},
        "test":{"target":"api:test","tags":["check-fast"],"options":{"runInCI":true}}},
      "cyc":{
        "a":{"target":"cyc:a","tags":[],"options":{"runInCI":true},
          "deps":[{"args":[],"env":{},"target":"cyc:b","cacheStrategy":"ignored"}]},
        "b":{"target":"cyc:b","tags":[],"options":{"runInCI":true},
          "deps":[{"args":[],"env":{},"target":"cyc:a","cacheStrategy":"ignored"}]}},
      "root":{
        "all":{"target":"root:all","tags":["check-fast"],"options":{"runInCI":true},
          "deps":[{"args":[],"env":{},"target":"api:build","cacheStrategy":"ignored"},
                  {"args":[],"env":{},"target":"alpha:serve","cacheStrategy":"ignored","type":"wait"},
                  {"args":[],"env":{},"target":"alpha:stop","cacheStrategy":"ignored","type":"cleanup"},
                  "ui:test",
                  7]}},
      "ui":{
        "test":{"target":"ui:test","tags":[],"options":{"runInCI":true}}}},
     "options":{"affected":null,"json":true}}"#;

    fn query() -> BTreeMap<String, Task> {
        query_tasks(&serde_json::from_str(QUERY).unwrap())
    }

    /// 149; C.2: a task's dependencies are each item of its `deps`, a
    /// bare target or an object's `target`, with or without a `type`, a
    /// `wait` one kept; a `cleanup` one, which moon runs after the task,
    /// and an item of any other shape are left out.
    #[test]
    fn a_tasks_dependencies_are_read_from_moon_query_tasks() {
        let tasks = query();
        assert_eq!(
            tasks["root:all"].deps,
            ["api:build", "alpha:serve", "ui:test"]
        );
        assert_eq!(tasks["api:build"].deps, ["api:test"]);
        assert!(tasks["ui:test"].deps.is_empty());
        assert!(tasks["alpha:stop"].deps.is_empty());
        assert_eq!(tasks["root:all"].tags, ["check-fast"]);
    }

    /// 149; design D1: each target's dependencies at any depth, each once
    /// and sorted; a cycle ends where it comes back, and a target with
    /// none is left out of the map.
    #[test]
    fn a_targets_dependencies_at_any_depth() {
        let tasks = query();
        let targets = ["api:build", "cyc:a", "root:all", "ui:test"].map(String::from);
        let deps = dependencies(&tasks, &targets);
        let s = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            deps,
            BTreeMap::from([
                ("api:build".to_string(), s(&["api:test"])),
                ("cyc:a".to_string(), s(&["cyc:b"])),
                (
                    "root:all".to_string(),
                    s(&["alpha:serve", "api:build", "api:test", "ui:test"])
                ),
            ])
        );
        assert!(dependencies(&tasks, &[]).is_empty());
    }

    /// An action as moon 2.6.1 writes it in its run report: a task that
    /// ran, with its hash and each try's exit; with no hash and no exit,
    /// one moon skipped, whose operations it writes as `[]` (#1305's
    /// `root:integration`).
    fn action(label: &str, status: &str, hash: Option<&str>, exits: &[i64]) -> Value {
        let target = label
            .split_once('(')
            .and_then(|(_, t)| t.strip_suffix(')'))
            .unwrap();
        let mut ops = Vec::new();
        if let Some(hash) = hash {
            ops.push(serde_json::json!({"status":"passed","meta":{"type":"hash-generation","hash":hash}}));
        }
        for exit in exits {
            let status = if *exit == 0 { "passed" } else { "failed" };
            ops.push(serde_json::json!({"status":status,
                "meta":{"type":"task-execution","command":"sh test.sh","exitCode":exit}}));
        }
        serde_json::json!({"allowFailure":false,"expectFailure":false,"flaky":false,
            "duration":{"secs":0,"nanos":34913000},"label":label,"status":status,
            "node":{"action":"run-task","params":{"args":[],"env":{},
                "persistent":label.starts_with("RunPersistentTask("),"target":target}},
            "operations":ops})
    }

    fn failed(target: &str, hash: &str) -> Value {
        action(&format!("RunTask({target})"), "failed", Some(hash), &[1])
    }

    fn passed(target: &str, hash: &str) -> Value {
        action(&format!("RunTask({target})"), "passed", Some(hash), &[0])
    }

    fn skipped(target: &str) -> Value {
        action(&format!("RunTask({target})"), "skipped", None, &[])
    }

    fn run_report(actions: Vec<Value>) -> Value {
        let mut all =
            vec![serde_json::json!({"label":"SyncWorkspace","status":"passed","operations":[]})];
        all.extend(actions);
        serde_json::json!({"actions":all,"status":"completed"})
    }

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(t, ds)| (t.to_string(), ds.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    fn read(report: &Value, targets: &[&str], deps: &[(&str, &[&str])], not_run: &[i32]) -> Read {
        let targets: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
        read_report(report, &targets, &graph(deps), not_run, "h")
    }

    /// Each check failed by a task, with the tasks it was failed by.
    fn by(r: &Read) -> Vec<(&str, Vec<&str>)> {
        r.failed_by
            .iter()
            .map(|f| (f.task.as_str(), f.by.iter().map(String::as_str).collect()))
            .collect()
    }

    fn named(results: &[TaskResult]) -> Vec<(&str, &str, Outcome)> {
        results
            .iter()
            .map(|t| (t.task.as_str(), t.hash.as_str(), t.outcome))
            .collect()
    }

    /// Nothing read of a target: no result, not run or failed by a
    /// dependency, so the check names it as left without a result (15).
    fn nothing(r: &Read) -> bool {
        r.tasks.is_empty()
            && r.not_run.is_empty()
            && r.failed_by.is_empty()
            && r.dependencies.is_empty()
    }

    /// 149; #1305's shape: a `noop` target moon skipped, `operations: []`,
    /// for the one task it depends on that ran and failed is failed by
    /// it, and that task's result, which is no target, is the run's, ran
    /// on this host with its hash, time and tries; a dependency that
    /// passed fails nothing.
    #[test]
    fn a_noop_target_skipped_for_its_failed_dependency_is_failed_by_it() {
        let report = run_report(vec![
            passed("tests/integration:ui", "u1"),
            failed("tests/integration:api", "8c41"),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[(
                "root:integration",
                &["tests/integration:api", "tests/integration:ui"],
            )],
            &[],
        );
        assert!(r.tasks.is_empty() && r.not_run.is_empty());
        assert_eq!(
            by(&r),
            [("root:integration", vec!["tests/integration:api"])]
        );
        assert_eq!(
            r.dependencies,
            [TaskResult {
                task: "tests/integration:api".into(),
                hash: "8c41".into(),
                outcome: Outcome::Fail,
                source: Source::Ran { host: "h".into() },
                took_ms: Some(34),
                attempts: Some(1),
            }]
        );
    }

    /// 149: a target is failed by the task that ran two steps down, and
    /// the task between, which moon skipped and the run does not require,
    /// is named nowhere; where it is a target too, it is failed by the
    /// same task, which is listed once.
    #[test]
    fn a_dependency_two_steps_down_fails_the_target() {
        let report = run_report(vec![
            failed("api:test", "t1"),
            action("RunTask(api:build)", "skipped", None, &[]),
            skipped("root:all"),
        ]);
        let r = read(
            &report,
            &["root:all"],
            &[("root:all", &["api:build", "api:test"])],
            &[],
        );
        assert_eq!(by(&r), [("root:all", vec!["api:test"])]);
        assert_eq!(named(&r.dependencies), [("api:test", "t1", Outcome::Fail)]);

        let r = read(
            &report,
            &["api:build", "root:all"],
            &[
                ("api:build", &["api:test"]),
                ("root:all", &["api:build", "api:test"]),
            ],
            &[],
        );
        assert_eq!(
            by(&r),
            [
                ("api:build", vec!["api:test"]),
                ("root:all", vec!["api:test"])
            ]
        );
        assert_eq!(named(&r.dependencies), [("api:test", "t1", Outcome::Fail)]);
    }

    /// 149: a target two of whose dependencies failed is failed by both,
    /// and each is listed once with its key.
    #[test]
    fn two_failed_dependencies_both_fail_the_target() {
        let report = run_report(vec![
            failed("ui:integration", "u1"),
            failed("api:integration", "a1"),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:integration", "ui:integration"])],
            &[],
        );
        assert_eq!(
            by(&r),
            [(
                "root:integration",
                vec!["api:integration", "ui:integration"]
            )]
        );
        assert_eq!(
            named(&r.dependencies),
            [
                ("api:integration", "a1", Outcome::Fail),
                ("ui:integration", "u1", Outcome::Fail)
            ]
        );
    }

    /// 149: a dependency that is itself a target is in `tasks` with its
    /// fail, and not in `dependencies`.
    #[test]
    fn a_dependency_that_is_a_target_is_listed_as_a_target() {
        let report = run_report(vec![failed("alpha:test", "a1"), skipped("beta:test")]);
        let r = read(
            &report,
            &["alpha:test", "beta:test"],
            &[("beta:test", &["alpha:test"])],
            &[],
        );
        assert_eq!(named(&r.tasks), [("alpha:test", "a1", Outcome::Fail)]);
        assert!(r.dependencies.is_empty());
        assert_eq!(by(&r), [("beta:test", vec!["alpha:test"])]);
    }

    /// 114, 149: a target skipped for a dependency whose exit `not_run`
    /// names is not run, saying which task could not run and why, and
    /// gives that task's result nowhere.
    #[test]
    fn a_dependency_that_could_not_run_leaves_the_target_not_run() {
        let report = run_report(vec![
            action("RunTask(api:integration)", "failed", Some("a1"), &[77]),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:integration"])],
            &[77],
        );
        assert!(r.tasks.is_empty() && r.dependencies.is_empty() && r.failed_by.is_empty());
        assert_eq!(
            r.not_run,
            [NotRun {
                task: "root:integration".into(),
                reason: "its dependency api:integration could not run: exited 77, \
                         which [tests] not_run names: it could not run here"
                    .into(),
                key: None,
            }]
        );
    }

    /// 149: one dependency that failed outweighs another that could not
    /// run: the target is failed by the first alone.
    #[test]
    fn a_failed_dependency_outweighs_one_that_could_not_run() {
        let report = run_report(vec![
            failed("api:integration", "a1"),
            action("RunTask(ui:integration)", "failed", Some("u1"), &[77]),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:integration", "ui:integration"])],
            &[77],
        );
        assert!(r.not_run.is_empty());
        assert_eq!(by(&r), [("root:integration", vec!["api:integration"])]);
        assert_eq!(
            named(&r.dependencies),
            [("api:integration", "a1", Outcome::Fail)]
        );
    }

    /// 15, 149: a dependency whose failure moon allows fails no target,
    /// so one skipped for another reason is left without a result.
    #[test]
    fn a_failure_moon_allows_fails_no_target() {
        let mut lint = failed("api:lint", "l1");
        lint["allowFailure"] = Value::Bool(true);
        let report = run_report(vec![lint, skipped("root:integration")]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:lint"])],
            &[],
        );
        assert!(nothing(&r), "{r:?}");
    }

    /// 15, 149: a target moon skipped while no task it depends on failed
    /// or could not run is left without a result.
    #[test]
    fn a_target_skipped_for_no_dependency_is_left_without_a_result() {
        let report = run_report(vec![
            passed("api:integration", "a1"),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:integration"])],
            &[],
        );
        assert!(nothing(&r), "{r:?}");
    }

    /// 149: a helper the target waits on is a task it depends on, so its
    /// failure fails the target, and its result is the run's.
    #[test]
    fn a_persistent_dependency_that_failed_fails_the_target() {
        let report = run_report(vec![
            action("RunPersistentTask(api:serve)", "failed", Some("s1"), &[1]),
            skipped("root:integration"),
        ]);
        let r = read(
            &report,
            &["root:integration"],
            &[("root:integration", &["api:serve"])],
            &[],
        );
        assert_eq!(by(&r), [("root:integration", vec!["api:serve"])]);
        assert_eq!(named(&r.dependencies), [("api:serve", "s1", Outcome::Fail)]);
    }

    /// 114, 119, 149: a dependency moon tried twice is judged by its last
    /// try: a not-run exit first and a fail last fails the target, with
    /// both tries counted; a fail first and a not-run exit last leaves
    /// the target not run.
    #[test]
    fn a_retried_dependency_is_judged_by_its_last_try() {
        let report = run_report(vec![
            action("RunTask(api:integration)", "failed", Some("a1"), &[77, 1]),
            skipped("root:integration"),
        ]);
        let deps: &[(&str, &[&str])] = &[("root:integration", &["api:integration"])];
        let r = read(&report, &["root:integration"], deps, &[77]);
        assert_eq!(by(&r), [("root:integration", vec!["api:integration"])]);
        assert_eq!(r.dependencies[0].attempts, Some(2));

        let report = run_report(vec![
            action("RunTask(api:integration)", "failed", Some("a1"), &[1, 77]),
            skipped("root:integration"),
        ]);
        let r = read(&report, &["root:integration"], deps, &[77]);
        assert!(r.failed_by.is_empty() && r.dependencies.is_empty());
        assert_eq!(r.not_run.len(), 1);
        assert!(
            r.not_run[0]
                .reason
                .starts_with("its dependency api:integration could not run: exited 77"),
            "{}",
            r.not_run[0].reason
        );
    }

    /// 15, 149; ADR 0044: a target the report does not name, as one moon
    /// keeps out of its CI mode is not, was skipped for nothing it depends
    /// on, though a task it depends on failed another target, and is left
    /// without a result.
    #[test]
    fn a_target_moon_never_skipped_is_read_through_nothing() {
        let report = run_report(vec![failed("alpha:test", "a1"), skipped("beta:test")]);
        let r = read(
            &report,
            &["app:serve", "beta:test"],
            &[
                ("app:serve", &["alpha:test"]),
                ("beta:test", &["alpha:test"]),
            ],
            &[],
        );
        assert_eq!(by(&r), [("beta:test", vec!["alpha:test"])]);
        assert_eq!(
            named(&r.dependencies),
            [("alpha:test", "a1", Outcome::Fail)]
        );
    }

    /// 15, 149: with no map, as a run an older sluice started has, a
    /// skipped target is read through nothing and left without a result.
    #[test]
    fn a_run_with_no_map_reads_a_skipped_target_as_before() {
        let report = run_report(vec![
            failed("api:integration", "a1"),
            skipped("root:integration"),
        ]);
        let r = read(&report, &["root:integration"], &[], &[]);
        assert!(nothing(&r), "{r:?}");
    }

    /// Checkpoints and run records as moon 2.6.1 writes them while a run
    /// goes (ADR 0028; the change's design, D3).
    #[test]
    fn a_runs_tasks_from_its_checkpoints_and_run_records() {
        let place =
            std::env::temp_dir().join(format!("sluice-moon-progress-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&place);
        let record = |target: &str, exit: i32, at: i64| {
            let dir = state_dir(&place, target).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("lastRun.json"),
                format!(r#"{{"exitCode":{exit},"hash":"","lastRunTime":{at},"outputGlobsHash":"","target":"{target}"}}"#),
            )
            .unwrap();
        };
        let start = 1_792_341_193_000;
        record("a:test", 0, start + 1_601);
        record("b:test", 0, start + 5_000);
        record("c:test", 3, start + 2_651);
        // From a run before this one: no end of this run's.
        record("d:test", 1, start - 60_000);
        record("e:test", 0, start + 900);
        let output = "\u{25AE}\u{25AE}\u{25AE}\u{25AE} a:test (f8bf095b)\n\
            \u{25AE}\u{25AE}\u{25AE}\u{25AE} c:test (3cf5763d)\n\
            \u{1b}[2m\u{25AE}\u{25AE}\u{25AE}\u{25AE}\u{1b}[0m \u{1b}[1mf:test\u{1b}[0m (0a1b2c3d)\n\
            a:test | \u{25AE}\u{25AE}\u{25AE}\u{25AE} g:test (1s, 99999999)\n\
            \u{25AE}\u{25AE}\u{25AE}\u{25AE} a:test (1s 65ms, f8bf095b)\n\
            \u{25AE}\u{25AE}\u{25AE}\u{25AE} e:test (cached, b7d5ce19)\n\
            \u{25AE}\u{25AE}\u{25AE}\u{25AE} c:test (2s 108ms, 3cf5763d)\n";
        let targets: Vec<String> = ["a:test", "c:test", "d:test", "e:test", "f:test", "g:test"]
            .map(String::from)
            .to_vec();
        let got = Moon.progress(&place, output.as_bytes(), &[], &targets, start);
        let short: Vec<(&str, TaskState, Option<&str>, Option<&str>, Option<i32>)> = got
            .iter()
            .map(|t| {
                (
                    t.target.as_str(),
                    t.state,
                    t.started.as_deref(),
                    t.ended.as_deref(),
                    t.exit,
                )
            })
            .collect();
        assert_eq!(
            short,
            vec![
                (
                    "a:test",
                    TaskState::Passed,
                    Some("2026-10-18T16:33:13Z"),
                    Some("2026-10-18T16:33:14Z"),
                    Some(0)
                ),
                (
                    "c:test",
                    TaskState::Failed,
                    Some("2026-10-18T16:33:13Z"),
                    Some("2026-10-18T16:33:15Z"),
                    Some(3)
                ),
                ("d:test", TaskState::Waiting, None, None, None),
                (
                    "e:test",
                    TaskState::Replayed,
                    None,
                    Some("2026-10-18T16:33:13Z"),
                    None
                ),
                ("f:test", TaskState::Running, None, None, None),
                ("g:test", TaskState::Waiting, None, None, None),
            ]
        );
        assert_eq!(duration_ms("2m 3s"), Some(123_000));
        assert_eq!(duration_ms("1s 65ms"), Some(1_065));
        assert_eq!(duration_ms("f8bf095b"), None);
        assert_eq!(duration_ms("cached"), None);
        std::fs::remove_dir_all(&place).unwrap();
    }

    /// ADR 0042: a running task started when its start line was printed,
    /// the first note of the output's length to reach that line's end; a
    /// run with no notes gives it no start.
    #[test]
    fn a_running_task_started_when_its_start_line_was_printed() {
        let place =
            std::env::temp_dir().join(format!("sluice-moon-started-{}", std::process::id()));
        let a = "\u{25AE}\u{25AE}\u{25AE}\u{25AE} a:test (f8bf095b)\n";
        let b = "\u{25AE}\u{25AE}\u{25AE}\u{25AE} b:test (3cf5763d)\n";
        let output = format!("{a}{b}");
        let times = [
            // Part of a's line, before the rest of it arrived.
            (a.len() as u64 - 3, "2026-10-21T12:00:00.250Z".to_string()),
            (a.len() as u64, "2026-10-21T12:00:01.500Z".to_string()),
            (output.len() as u64, "2026-10-21T12:04:00.000Z".to_string()),
        ];
        let targets = ["a:test", "b:test", "c:test"].map(String::from).to_vec();
        let started = |times: &[(u64, String)]| -> Vec<(TaskState, Option<String>)> {
            Moon.progress(&place, output.as_bytes(), times, &targets, 0)
                .into_iter()
                .map(|t| (t.state, t.started))
                .collect()
        };
        assert_eq!(
            started(&times),
            [
                (TaskState::Running, Some("2026-10-21T12:00:01Z".to_string())),
                (TaskState::Running, Some("2026-10-21T12:04:00Z".to_string())),
                (TaskState::Waiting, None),
            ]
        );
        assert_eq!(
            started(&[]),
            [
                (TaskState::Running, None),
                (TaskState::Running, None),
                (TaskState::Waiting, None),
            ]
        );
    }

    /// Time-limit-1 66: a task the limit cut off is named however moon
    /// read it: failed with the exit it took on the signal, as on a loaded
    /// host, skipped, or started with no run record at all. What ended
    /// before the limit stands, a fail too, and a replay or a task never
    /// started is not named.
    #[test]
    fn a_task_the_limit_cut_off_is_named_however_moon_read_it() {
        let place =
            std::env::temp_dir().join(format!("sluice-moon-cut-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&place);
        let record = |target: &str, exit: i32, at: i64| {
            let dir = state_dir(&place, target).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("lastRun.json"),
                format!(r#"{{"exitCode":{exit},"hash":"","lastRunTime":{at},"outputGlobsHash":"","target":"{target}"}}"#),
            )
            .unwrap();
        };
        let start = 1_792_341_193_000;
        let limit = start + 3_600_000;
        record("a:test", 0, start + 1_000);
        record("b:test", 3, start + 2_000);
        record("c:test", 143, limit + 40);
        record("d:test", -1, limit + 60);
        let report: Value = serde_json::from_str(
            r#"{"status":"interrupted","actions":[
              {"label":"RunTask(a:test)","status":"passed","node":{"params":{"target":"a:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h1"}},
                             {"status":"passed","meta":{"type":"task-execution","exitCode":0}}]},
              {"label":"RunTask(b:test)","status":"failed","node":{"params":{"target":"b:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h2"}},
                             {"status":"failed","meta":{"type":"task-execution","exitCode":3}}]},
              {"label":"RunTask(c:test)","status":"failed","node":{"params":{"target":"c:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h3"}},
                             {"status":"failed","meta":{"type":"task-execution","exitCode":143}}]},
              {"label":"RunTask(d:test)","status":"skipped","node":{"params":{"target":"d:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h4"}},
                             {"status":"failed","meta":{"type":"task-execution"}}]},
              {"label":"RunTask(e:test)","status":"cached","node":{"params":{"target":"e:test"}},
               "operations":[{"status":"passed","meta":{"type":"hash-generation","hash":"h5"}},
                             {"status":"cached","meta":{"type":"output-hydration"}}]},
              {"label":"RunTask(g:test)","status":"skipped","node":{"params":{"target":"g:test"}},"operations":[]}
            ]}"#,
        )
        .unwrap();
        let targets: Vec<String> = [
            "a:test", "b:test", "c:test", "d:test", "e:test", "f:test", "g:test",
        ]
        .map(String::from)
        .to_vec();
        let results = read_report(&report, &targets, &BTreeMap::new(), &[], "h").tasks;
        let output = "\u{25AE}\u{25AE}\u{25AE}\u{25AE} g:test (0a1b2c3d)\n";
        let cut = Moon.cut_off(&place, output.as_bytes(), &targets, &results, start, limit);
        assert_eq!(cut, ["c:test", "d:test", "g:test"]);
        std::fs::remove_dir_all(&place).unwrap();
    }

    /// moon 2.6.1's own names, as `encode_component` makes them.
    #[test]
    fn state_dirs_as_moon_names_them() {
        let dir = |target: &str| {
            state_dir(Path::new("/p"), target)
                .map(|d| d.strip_prefix("/p/.moon/cache/states").unwrap().to_owned())
        };
        assert_eq!(dir("a:test"), Some("a/test".into()));
        assert_eq!(
            dir("cdk/archetypes:test"),
            Some("cdk-archetypes/test".into())
        );
        assert_eq!(dir("@scope/pkg:build"), Some("scope-pkg/build".into()));
        assert_eq!(dir("a:lint/fix"), Some("a/lint-fix".into()));
        assert_eq!(dir(".x.:test"), Some("x/test".into()));
        assert_eq!(dir("notarget"), None);
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("moon 2.6.1\n"), Some((2, 6)));
        assert_eq!(parse_version("moon 1.41.7"), Some((1, 41)));
        assert!(parse_version("moon").is_none());
        assert!((2, 5) < FLOOR && (2, 6) >= FLOOR && (3, 0) > FLOOR && (2, 10) > FLOOR);
    }

    /// 147: a working copy of a test's own, with a stand-in `moon` first
    /// on the path that runs `moon`, and a request whose place it is.
    struct Copy {
        dir: PathBuf,
        request: sluice_seams::RunRequest,
        env: BTreeMap<String, String>,
    }

    impl Copy {
        fn new(name: &str, moon: &str) -> Copy {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = std::env::temp_dir()
                .join(format!("sluice-moon-step-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let stand_in = bin.join("moon");
            std::fs::write(&stand_in, format!("#!/bin/sh\n{moon}\n")).unwrap();
            std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755)).unwrap();
            let request = sluice_seams::RunRequest {
                place: sluice_seams::Place {
                    name: "equip".into(),
                    path: dir.clone(),
                    tmp: dir.join("tmp"),
                    env: BTreeMap::new(),
                },
                revision: Rev::new(""),
                required: Vec::new(),
                since: None,
                limit: None,
                limits: sluice_seams::Limits::default(),
                tests: sluice_seams::Settings::new(),
                command: None,
                replay: false,
                system: None,
            };
            let env = BTreeMap::from([(
                "PATH".to_string(),
                format!("{}:/usr/bin:/bin", bin.display()),
            )]);
            Copy { dir, request, env }
        }

        fn cx(&self) -> Context<'_> {
            Context {
                request: &self.request,
                env: &self.env,
                toolchain: None,
                jobs: 1,
            }
        }

        /// The step `name`, running `script` by `sh`.
        fn step(&self, name: &str, script: &str) -> EquipStep {
            Moon.step(&self.cx(), name, &["/bin/sh", "-c", script]).0
        }
    }

    impl Drop for Copy {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 147: a step's words are what it printed, its output then its error
    /// stream, each trimmed; one that failed is given its exit before them.
    #[test]
    fn a_step_that_exited_is_given_its_exit_then_what_it_printed() {
        let copy = Copy::new("exited", "");
        assert_eq!(
            copy.step(
                "moon setup",
                "echo ' out '; echo 'error: no such toolchain' >&2; exit 3"
            ),
            EquipStep {
                step: "moon setup".into(),
                passed: false,
                words: "exited 3: out\nerror: no such toolchain".into(),
            }
        );
        assert_eq!(
            copy.step("moon setup", "echo set up; echo warned >&2"),
            EquipStep {
                step: "moon setup".into(),
                passed: true,
                words: "set up\nwarned".into(),
            }
        );
    }

    /// 147: a step that failed printing nothing is given its exit alone.
    #[test]
    fn a_step_that_printed_nothing_is_given_its_exit_alone() {
        let copy = Copy::new("silent", "");
        let step = copy.step("moon setup", "exit 1");
        assert!(!step.passed);
        assert_eq!(step.words, "exited 1");
    }

    /// 147: a step a signal ended says which.
    #[test]
    fn a_step_ended_by_a_signal_says_which() {
        let copy = Copy::new("signal", "");
        let step = copy.step("moon setup", "kill -9 $$");
        assert!(!step.passed);
        assert_eq!(step.words, "ended by signal 9");
    }

    /// 147: a step whose program could not be started says why.
    #[test]
    fn a_step_whose_program_could_not_be_run_says_why() {
        let copy = Copy::new("unrun", "");
        let missing = copy.dir.join("no-such-proto");
        let (step, out) = Moon.step(&copy.cx(), "proto install", &[missing.to_str().unwrap()]);
        assert!(out.is_none());
        assert!(!step.passed);
        assert!(
            step.words.starts_with("could not be run: "),
            "{}",
            step.words
        );
    }

    /// 147, C.2: a moon below 2.6 fails the version check, in words that
    /// name it and the oldest the runner reads.
    #[test]
    fn a_moon_below_the_floor_fails_its_version_check() {
        let copy = Copy::new("old", "echo 'moon 2.5.3'");
        assert_eq!(
            Moon.check_version(&copy.cx()),
            EquipStep {
                step: "moon --version".into(),
                passed: false,
                words: "moon 2.5.3 is below moon 2.6, the oldest the moon runner reads".into(),
            }
        );
        let copy = Copy::new("new", "echo 'moon 2.6.1'");
        assert!(Moon.check_version(&copy.cx()).passed);
    }

    /// 147: a version moon cannot say fails the version check, quoting
    /// what it said, and what it printed on its error stream after it.
    #[test]
    fn a_version_that_cannot_be_read_fails_its_version_check() {
        let copy = Copy::new("unread", "echo 'not a version'; echo 'no workspace' >&2");
        assert_eq!(
            Moon.check_version(&copy.cx()).words,
            r#"moon --version said "not a version": no workspace"#
        );
    }
}
