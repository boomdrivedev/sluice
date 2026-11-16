//! A combination under test as a person reads it: how many of its tasks
//! have ended of how many, the tasks running and for how long, and its
//! time against its limit (ADR 0042). Read from what the overview reads
//! (ADR 0028); it decides nothing and writes nothing (rules core.11,
//! core.14).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use sluice_core::overview::{BranchView, CandidateView};
use sluice_core::records::{CandidateState, LostRun};
use sluice_core::Sluice;
use sluice_seams::TaskState;

use crate::{combination, Names};

/// How many running tasks a line names.
const NAMED: usize = 3;

/// `testing main + #12 · 7/12 tasks · running a:test 4m02s · 6m10s of
/// 30m`, for each combination of `branch` under test; with `only`, the
/// one submission `only`'s: the combination whose last submission it is,
/// else any that holds it. Its submissions by `names`, where given.
pub fn lines(
    s: &Sluice<'_>,
    branch: &str,
    only: Option<u64>,
    now: DateTime<Utc>,
    names: Option<&Names>,
) -> Result<Vec<String>> {
    let view = s.branch_view_of(branch)?;
    let shown: Vec<&CandidateView> = match only {
        None => under_test(&view).collect(),
        Some(id) => own(&view, id).into_iter().collect(),
    };
    Ok(shown
        .into_iter()
        .map(|c| testing_line(branch, c, now, names))
        .collect())
}

/// The combinations of a branch under test.
fn under_test(view: &BranchView) -> impl Iterator<Item = &CandidateView> {
    view.candidates
        .iter()
        .filter(|c| matches!(c.state, CandidateState::Testing))
}

/// Submission `id`'s combination under test: the one whose last
/// submission it is, else any that holds it.
pub fn own(view: &BranchView, id: u64) -> Option<&CandidateView> {
    under_test(view)
        .find(|c| c.submissions.last() == Some(&id))
        .or_else(|| under_test(view).find(|c| c.submissions.contains(&id)))
}

/// What a waiting command prints a line again for: the combination, how
/// many of its tasks have ended, which are running, what it waits for and
/// why its last run had no outcome; never its times.
pub fn moved(c: &CandidateView) -> Moved {
    let tasks = c.tasks.as_deref().unwrap_or_default();
    (
        c.submissions.clone(),
        tasks.iter().filter(|t| t.state.ended()).count(),
        tasks
            .iter()
            .filter(|t| t.state == TaskState::Running)
            .map(|t| t.target.clone())
            .collect(),
        c.waiting.clone(),
        c.lost.clone(),
    )
}

/// What [`moved`] reads of a combination.
pub type Moved = (
    Vec<u64>,
    usize,
    Vec<String>,
    BTreeMap<String, String>,
    Vec<LostRun>,
);

/// 84, 87, record 0074: `waiting for a host that can run its tests as its
/// policy asks: build-box: no container engine answers: no socket at
/// /var/run/docker.sock`, for a combination with a run some host turned
/// down, for its engine or a limit its executor cannot hold, and none has
/// taken; `None` for one that waits for nothing.
pub fn waiting(c: &CandidateView) -> Option<String> {
    if c.waiting.is_empty() {
        return None;
    }
    let hosts: Vec<String> = c.waiting.iter().map(|(h, w)| format!("{h}: {w}")).collect();
    Some(format!(
        "waiting for a host that can run its tests as its policy asks: {}",
        hosts.join("; ")
    ))
}

/// 114, record 0105 (design D7 of the change
/// `a-pass-nothing-proves-blames-no-one`): `its last run, on build-1, had
/// no outcome: a:test, b:test, c:test and 25 more could not run: <why>;
/// d:test could not run: <other>`, for each run of a combination tested
/// again after a run that left checks not run and judged nothing, whatever
/// the cause, its system after its host where the policy names one, the
/// runs joined by ` · `; `None` for one whose runs each had an outcome.
pub fn lost(c: &CandidateView) -> Option<String> {
    if c.lost.is_empty() {
        return None;
    }
    let runs: Vec<String> = c
        .lost
        .iter()
        .map(|run| {
            let host = match &run.system {
                Some(system) => format!("{} ({system})", run.host),
                None => run.host.clone(),
            };
            let mut reasons: Vec<(&str, Vec<&str>)> = Vec::new();
            for n in &run.not_run {
                match reasons.iter_mut().find(|(why, _)| *why == n.reason) {
                    Some((_, tasks)) => tasks.push(&n.task),
                    None => reasons.push((&n.reason, vec![&n.task])),
                }
            }
            let parts: Vec<String> = reasons
                .iter()
                .map(|(why, tasks)| {
                    let mut named = tasks
                        .iter()
                        .take(NAMED)
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ");
                    if tasks.len() > NAMED {
                        named.push_str(&format!(" and {} more", tasks.len() - NAMED));
                    }
                    format!("{named} could not run: {why}")
                })
                .collect();
            format!(
                "its last run, on {host}, had no outcome: {}",
                parts.join("; ")
            )
        })
        .collect();
    Some(runs.join(" · "))
}

/// 84, record 0074: what each combination of `view` under test waits for,
/// for `--json`: its submissions, its revision and the hosts that turned
/// its run down, each with why.
pub fn waiting_json(view: &BranchView) -> serde_json::Value {
    under_test(view)
        .filter(|c| !c.waiting.is_empty())
        .map(|c| {
            serde_json::json!({
                "submissions": c.submissions,
                "combination": c.revision,
                "waiting": c.waiting,
            })
        })
        .collect()
}

/// `testing <combination>`, and its progress where there is any; the
/// combination's submissions by `names`, where given (26), and by their
/// numbers alone otherwise, as `sluice wait` says them.
pub fn testing_line(
    branch: &str,
    c: &CandidateView,
    now: DateTime<Utc>,
    names: Option<&Names>,
) -> String {
    let named = combination(branch, &c.submissions, names, &mut BTreeSet::new());
    let p = [Some(line(c, now)), waiting(c), lost(c)]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    match p {
        p if p.is_empty() => format!("testing {named}"),
        p => format!("testing {named} · {p}"),
    }
}

/// `7/12 tasks · running a:test 4m02s, b:test 40s · 6m10s of 30m`: the
/// tasks where its runner says, up to [`NAMED`] running ones, longest
/// first, and the combination's time since its testing line; its time
/// alone where no tasks are kept.
pub fn line(c: &CandidateView, now: DateTime<Utc>) -> String {
    let mut parts = Vec::new();
    if let Some(tasks) = c.tasks.as_ref().filter(|t| !t.is_empty()) {
        let done = tasks.iter().filter(|t| t.state.ended()).count();
        parts.push(format!("{done}/{} tasks", tasks.len()));
        let mut running: Vec<(&str, Option<i64>)> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Running)
            .map(|t| (t.target.as_str(), since(t.started.as_deref(), now)))
            .collect();
        // Longest first; one with no start after those with one, each in
        // the runner's order.
        running.sort_by_key(|(_, s)| std::cmp::Reverse(s.unwrap_or(-1)));
        if !running.is_empty() {
            let mut named: Vec<String> = running
                .iter()
                .take(NAMED)
                .map(|(t, s)| match s {
                    Some(s) => format!("{t} {}", duration(*s)),
                    None => t.to_string(),
                })
                .collect();
            if running.len() > NAMED {
                named.push(format!("+{} more", running.len() - NAMED));
            }
            parts.push(format!("running {}", named.join(", ")));
        }
    }
    if let Some(s) = since(c.started.as_deref(), now) {
        parts.push(match c.limit {
            Some(l) => format!("{} of {}", duration(s), limit(l)),
            None => duration(s),
        });
    }
    parts.join(" · ")
}

/// Whole seconds from `at` to `now`; none for a time that cannot be read.
fn since(at: Option<&str>, now: DateTime<Utc>) -> Option<i64> {
    let at = DateTime::parse_from_rfc3339(at?).ok()?;
    Some((now - at.with_timezone(&Utc)).num_seconds().max(0))
}

/// `45s`, `2m10s`, `1h03m`, as the board says a time.
pub fn duration(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// `15m` for a limit of whole minutes under an hour, else a [`duration`].
fn limit(seconds: u64) -> String {
    let s = seconds as i64;
    if s % 60 == 0 && s < 3600 {
        format!("{}m", s / 60)
    } else {
        duration(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sluice_seams::{Rev, TaskProgress};

    fn now() -> DateTime<Utc> {
        "2026-10-21T12:30:00Z".parse().unwrap()
    }

    fn task(target: &str, state: TaskState, started: Option<&str>) -> TaskProgress {
        TaskProgress {
            target: target.into(),
            state,
            started: started.map(String::from),
            ended: None,
            exit: None,
        }
    }

    fn candidate(tasks: Option<Vec<TaskProgress>>, limit: Option<u64>) -> CandidateView {
        CandidateView {
            id: "c1".into(),
            submissions: vec![12],
            revision: Rev("0123456789abcdef0123456789abcdef01234567".into()),
            on: None,
            state: CandidateState::Testing,
            started: Some("2026-10-21T12:03:50Z".into()),
            limit,
            targets: Vec::new(),
            executor: None,
            ran: 0,
            replayed: 0,
            failed: Vec::new(),
            tasks,
            waiting: BTreeMap::new(),
            lost: Vec::new(),
        }
    }

    #[test]
    fn a_count_running_tasks_and_the_time() {
        let tasks = vec![
            task("a:test", TaskState::Passed, None),
            task(
                "site:prose",
                TaskState::Running,
                Some("2026-10-21T12:29:20Z"),
            ),
            task(
                "sluice:e2e",
                TaskState::Running,
                Some("2026-10-21T12:04:57Z"),
            ),
            task("c:test", TaskState::Waiting, None),
        ];
        assert_eq!(
            line(&candidate(Some(tasks), Some(2700)), now()),
            "1/4 tasks · running sluice:e2e 25m03s, site:prose 40s · 26m10s of 45m"
        );
    }

    #[test]
    fn more_than_three_running_and_one_with_no_start() {
        let tasks = vec![
            task("a", TaskState::Running, None),
            task("b", TaskState::Running, Some("2026-10-21T12:29:00Z")),
            task("c", TaskState::Running, Some("2026-10-21T12:28:00Z")),
            task("d", TaskState::Running, Some("2026-10-21T12:27:00Z")),
            task("e", TaskState::Replayed, None),
        ];
        assert_eq!(
            line(&candidate(Some(tasks), None), now()),
            "1/5 tasks · running d 3m00s, c 2m00s, b 1m00s, +1 more · 26m10s"
        );
        let tasks = vec![task("a", TaskState::Running, None)];
        assert_eq!(
            line(&candidate(Some(tasks), Some(900)), now()),
            "0/1 tasks · running a · 26m10s of 15m"
        );
    }

    #[test]
    fn no_tasks_kept_gives_the_time_alone() {
        assert_eq!(line(&candidate(None, Some(5400)), now()), "26m10s of 1h30m");
        let mut c = candidate(None, None);
        c.started = None;
        assert_eq!(line(&c, now()), "");
        assert_eq!(testing_line("main", &c, now(), None), "testing main + #12");
    }

    /// 84, record 0074: a combination whose run a host turned down for
    /// its engine says what it waits for, after its time.
    #[test]
    fn a_run_turned_down_for_its_engine_says_what_it_waits_for() {
        let mut c = candidate(None, Some(1800));
        c.waiting = BTreeMap::from([
            (
                "box".into(),
                "no container engine answers: /var/run/docker.sock refused the connection".into(),
            ),
            (
                "build-box".into(),
                "[executor] memory = 4294967296 bytes: the sandbox executor on macOS cannot hold \
                 a memory limit"
                    .into(),
            ),
        ]);
        assert_eq!(
            testing_line("main", &c, now(), None),
            "testing main + #12 · 26m10s of 30m · waiting for a host that can run its tests as \
             its policy asks: box: no container engine answers: /var/run/docker.sock refused the \
             connection; build-box: [executor] memory = 4294967296 bytes: the sandbox executor \
             on macOS cannot hold a memory limit"
        );
        c.started = None;
        assert_eq!(
            testing_line("main", &c, now(), None),
            "testing main + #12 · waiting for a host that can run its tests as its policy asks: \
             box: no container engine answers: /var/run/docker.sock refused the connection; \
             build-box: [executor] memory = 4294967296 bytes: the sandbox executor on macOS \
             cannot hold a memory limit"
        );
    }

    /// 114, record 0105 (design D7; the run-outcome spec's "Many tests
    /// under one reason"): a combination tested again after a run that had
    /// no outcome says on which host, each reason once with up to three of
    /// its checks in the run's order and how many more, and a run on a
    /// system names it after its host.
    #[test]
    fn a_combination_whose_last_run_had_no_outcome_says_why() {
        let gone = "it passed, but nothing the run kept proves it ran: \
                    moon's manifest of it is not in /repo/.moon/cache";
        let mut not_run: Vec<sluice_seams::NotRun> = (1..=28)
            .map(|n| sluice_seams::NotRun {
                task: format!("p{n:02}:test"),
                reason: gone.into(),
                key: None,
            })
            .collect();
        not_run.insert(
            1,
            sluice_seams::NotRun {
                task: "e2e:test".into(),
                reason: "exited 77, which [tests] not_run names: it could not run here".into(),
                key: None,
            },
        );
        let mut c = candidate(None, Some(1800));
        c.lost = vec![LostRun {
            host: "build-1".into(),
            system: None,
            not_run,
        }];
        assert_eq!(
            testing_line("main", &c, now(), None),
            format!(
                "testing main + #12 · 26m10s of 30m · its last run, on build-1, had no \
                 outcome: p01:test, p02:test, p03:test and 25 more could not run: {gone}; \
                 e2e:test could not run: exited 77, which [tests] not_run names: it could not \
                 run here"
            )
        );
        c.started = None;
        c.lost.push(LostRun {
            host: "mac-1".into(),
            system: Some("macos".into()),
            not_run: vec![sluice_seams::NotRun {
                task: "a:test".into(),
                reason: "the engine is down".into(),
                key: None,
            }],
        });
        assert!(
            testing_line("main", &c, now(), None).ends_with(
                " · its last run, on mac-1 (macos), had no outcome: a:test could not run: \
                 the engine is down"
            ),
            "{}",
            testing_line("main", &c, now(), None)
        );
        let before = moved(&c);
        c.lost.pop();
        assert_ne!(moved(&c), before, "a wait prints the line again");
    }

    #[test]
    fn durations_read_as_the_boards() {
        assert_eq!(duration(-3), "0s");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(130), "2m10s");
        assert_eq!(duration(3780), "1h03m");
        assert_eq!(limit(900), "15m");
        assert_eq!(limit(3600), "1h00m");
    }
}
