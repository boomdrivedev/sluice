//! A submission's report as a pull request shows it (ADR 0044): one
//! comment, the `sluice` check's text, and a check per required check per
//! system. Drawn from the core's [`VerdictView`] alone, the same whatever
//! task runner wrote the results, in the layout CI reports on pull
//! requests have taught their readers: a head line, the totals, and one
//! row per check. Deterministic, so a report that has not changed is not
//! posted again.

use std::collections::{BTreeMap, BTreeSet};

use sluice_core::verdict_view::{Row, RowResult, Standing, VerdictView};
use sluice_seams::{Enforced, TaskProgress, TaskState};

use super::forge::{check_name, Posting, Status};
use crate::scrub::{last_lines, Scrub};

/// GitHub's most for a comment's body or a check's text.
const MOST: usize = 65_000;

/// Rows shown before the rest are folded away.
const SHOWN: usize = 20;

/// A task over this many milliseconds is flagged slow.
const SLOW_MS: u64 = 120_000;

/// What drawing a report needs beyond the view.
pub struct Context<'a> {
    /// How each submission is named: `#12` for a pull request.
    pub names: &'a BTreeMap<u64, String>,
    /// The repository's page, `https://github.com/o/r`, to link revisions
    /// the forge holds.
    pub repo_url: String,
    /// The branch policy's `pull_requests.log_lines`.
    pub log_lines: Option<usize>,
    /// What a failed check printed, read where the records say it is
    /// kept; never fetched.
    pub read_log: &'a dyn Fn(&str) -> Option<String>,
    pub scrub: &'a Scrub,
}

impl Context<'_> {
    fn name(&self, id: u64) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("submission {id}"))
    }
}

/// The marker that finds the report's comment again.
pub fn marker(branch: &str) -> String {
    format!("<!-- sluice-report: {branch} -->")
}

/// Whether `view` shows anything a comment carries: a combination tested,
/// one formed again after it was, or a landing beside the result that
/// covers it (140; record 0083).
pub fn has_report(view: &VerdictView) -> bool {
    !view.rows.is_empty()
        || !view.tasks.is_empty()
        || view.standing == Standing::FormedAgain
        || matches!(
            view.standing,
            Standing::Landed {
                beside: true,
                covered_by: Some(_),
                ..
            }
        )
}

/// The head line: the branch, where the submission stands, and the blamed
/// pull request when there is one.
pub fn head(view: &VerdictView, cx: &Context) -> String {
    let b = &view.branch;
    match &view.standing {
        Standing::Queued { ahead } if ahead.is_empty() => format!("queued on {b}, next"),
        Standing::Queued { ahead } => format!(
            "queued on {b} behind {}",
            ahead
                .iter()
                .map(|i| cx.name(*i))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Standing::Waiting => format!("waiting on {b}"),
        Standing::Testing => format!("testing on {b}"),
        Standing::Passed => format!("passed on {b}, waiting on the combinations ahead"),
        Standing::Landing => format!("passed on {b}, landing"),
        Standing::Refused { words } => {
            format!("passed on {b}, held: the forge refused its landing: {words}")
        }
        Standing::Landed {
            landed_as,
            beside: false,
            ..
        } => format!("landed on {b} as {}", landed_as.short()),
        // 140: nothing was run, so the report has no rows.
        Standing::Landed {
            landed_as,
            beside: true,
            ..
        } => format!(
            "landed on {b} as {} with no check run: no required check reads its changes",
            landed_as.short()
        ),
        Standing::Blamed { timed_out: true } => {
            format!("timed out on {b}, {} is to blame", cx.name(view.id))
        }
        Standing::Blamed { timed_out: false } => {
            format!("failed on {b}, {} is to blame", cx.name(view.id))
        }
        Standing::Conflict { .. } => format!("conflict on {b}"),
        Standing::FormedAgain => format!("formed again on {b}"),
        Standing::Withdrawn => format!("withdrawn from {b}"),
    }
}

/// The comment and the `sluice` check's text: the whole report.
pub fn report(view: &VerdictView, cx: &Context) -> String {
    let full = draw(view, cx, Fit::Whole);
    if full.len() <= MOST {
        return full;
    }
    let lean = draw(view, cx, Fit::NoOutput);
    if lean.len() <= MOST {
        return lean;
    }
    draw(view, cx, Fit::Rows)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Whole,
    /// Without the output and the changed files.
    NoOutput,
    /// Only the first rows.
    Rows,
}

fn draw(view: &VerdictView, cx: &Context, fit: Fit) -> String {
    let mut s = format!("{}\n## sluice: {}\n", marker(&view.branch), head(view, cx));
    if view.standing == Standing::FormedAgain {
        s.push_str(
            "\nIts combination is formed again and will be tested again. \
             Nothing of the one it left is shown here.\n",
        );
        return s;
    }
    // 23, 140: a landing beside names the result that covers it, as its
    // landing commit does, and no row, since no check was run or replayed
    // on it (118; record 0083).
    if let Standing::Landed {
        beside: true,
        covered_by: Some(c),
        ..
    } = &view.standing
    {
        s.push_str(&format!(
            "\nCovered by the result at [`{}`]({}/commit/{}), {}.\n",
            c.at.short(),
            cx.repo_url,
            c.at,
            sluice_core::text::verdict_words(&c.result, c.ran, c.replayed)
        ));
        return s;
    }
    if let (Some(c), Some(on)) = (&view.combination, &view.on) {
        let mut line = format!(
            "\nTested `{}` = {} [`{}`]({}/commit/{on})",
            c.short(),
            view.branch,
            on.short(),
            cx.repo_url
        );
        for id in &view.submissions {
            line.push_str(&format!(" + {}", cx.name(*id)));
        }
        let ran = view
            .rows
            .iter()
            .filter(|r| r.key.is_some() && !r.replayed)
            .count();
        let replayed = view.rows.iter().filter(|r| r.replayed).count();
        if !view.rows.is_empty() {
            line.push_str(&format!(" · {ran} ran, {replayed} replayed"));
        }
        s.push_str(&line);
        s.push('\n');
    }
    for r in &view.runs {
        let d = &r.detail;
        let mut parts = Vec::new();
        if let Some(t) = d.took_ms {
            parts.push(format!("Total time: {}", time(t)));
        }
        if let Some(e) = d.estimate_ms {
            parts.push(format!("Comparison time: {}", time(e)));
        }
        if let (Some(saved @ 1..), Some(e)) = (d.saved_ms(), d.estimate_ms) {
            let pct = (saved * 100).checked_div(e).unwrap_or(0);
            parts.push(format!(
                "Estimated savings: {} ({pct}% faster)",
                time(saved)
            ));
        }
        if parts.is_empty() {
            continue;
        }
        let on = r
            .system
            .as_ref()
            .map(|s| format!("{s}: "))
            .unwrap_or_default();
        s.push_str(&format!("\n{on}{}\n", parts.join(" | ")));
    }
    if !view.rows.is_empty() {
        s.push_str(&table(&view.rows, fit));
    } else if !view.tasks.is_empty() {
        s.push_str(&live(&view.tasks));
    }
    let executors: BTreeSet<String> = view
        .rows
        .iter()
        .filter_map(|r| r.executor.as_ref().map(executor))
        .collect();
    if !executors.is_empty() {
        s.push_str(&details(
            "Executor",
            &executors.into_iter().collect::<Vec<_>>().join("\n\n"),
        ));
    }
    if fit == Fit::Whole {
        let changed: BTreeSet<&String> = view.runs.iter().flat_map(|r| &r.detail.changed).collect();
        if !changed.is_empty() {
            let mut list: Vec<String> = changed
                .iter()
                .take(100)
                .map(|f| format!("- `{f}`"))
                .collect();
            if changed.len() > 100 {
                list.push(format!("- and {} more", changed.len() - 100));
            }
            s.push_str(&details("Changed files", &list.join("\n")));
        }
        s.push_str(&outputs(&view.rows, cx));
    }
    s.push_str(&kept(&view.rows, cx));
    s
}

/// The rows, failed first, the first [`SHOWN`] open and the rest folded.
fn table(rows: &[Row], fit: Fit) -> String {
    let header = "\n|  | Check | Time | Result | Info |\n|---|---|---|---|---|\n";
    let mut s = String::from(header);
    for r in rows.iter().take(SHOWN) {
        s.push_str(&row_line(r));
    }
    let rest = rows.len().saturating_sub(SHOWN);
    if rest > 0 {
        if fit == Fit::Rows {
            s.push_str(&format!(
                "\n{rest} more checks, every one shown by `sluice status`.\n"
            ));
        } else {
            let mut more = String::from(header.trim_start());
            for r in rows.iter().skip(SHOWN) {
                more.push_str(&row_line(r));
            }
            s.push_str(&details(&format!("Every check ({rest} more)"), &more));
        }
    }
    s
}

fn row_line(r: &Row) -> String {
    format!(
        "| {} | {} | {} | {} | {} |\n",
        icon(r),
        check(r),
        r.took_ms.map(time).unwrap_or_default(),
        result_word(r),
        info(r, true)
    )
}

fn check(r: &Row) -> String {
    match &r.system {
        Some(s) => format!("`{}` ({s})", r.task),
        None => format!("`{}`", r.task),
    }
}

fn icon(r: &Row) -> &'static str {
    match r.result {
        RowResult::Passed if r.replayed => "🟪",
        RowResult::Passed => "🟩",
        RowResult::Failed => "🟥",
        RowResult::Flaky => "🟨",
        RowResult::NotRun | RowResult::Missing | RowResult::TimedOut | RowResult::SetAside => "⬛",
    }
}

fn result_word(r: &Row) -> &'static str {
    match r.result {
        RowResult::Passed if r.replayed => "Replayed",
        RowResult::Passed => "Passed",
        RowResult::Failed => "Failed",
        RowResult::Flaky => "Flaky",
        RowResult::NotRun => "Could not run",
        RowResult::Missing => "Missing",
        RowResult::TimedOut => "Timed out",
        RowResult::SetAside => "Set aside",
    }
}

/// Where and how it ran, and anything to note: in markdown, or plain
/// for a check's title, which GitHub shows as it is.
fn info(r: &Row, markdown: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if r.replayed {
        parts.push("from a pass the gate trusts".into());
    } else if let Some(h) = &r.host {
        let how = r
            .executor
            .as_ref()
            .map(|e| format!("{} on {h}", e.kind))
            .unwrap_or_else(|| format!("on {h}"));
        parts.push(how);
    }
    if let Some(s) = &r.signer {
        parts.push(format!("signed by {s}"));
    }
    if let Some(n) = r.attempts.filter(|n| *n > 1) {
        parts.push(format!("{n} attempts"));
    }
    if r.took_ms.is_some_and(|t| t > SLOW_MS) {
        parts.push(if markdown { "**slow**" } else { "slow" }.into());
    }
    if let Some(k) = &r.flaky {
        parts.push(format!(
            "{} on the same inputs at {}",
            crate::words::outcome(k.outcome),
            k.at
        ));
    }
    if let Some(why) = &r.not_run {
        parts.push(cell(why));
    }
    // 149: the tasks it depends on that failed it, each with a row of its
    // own; in a check's title, which GitHub shows as it is, unquoted.
    if !r.failed_by.is_empty() {
        let named: Vec<String> = r
            .failed_by
            .iter()
            .map(|t| match markdown {
                true => format!("`{t}`"),
                false => t.clone(),
            })
            .collect();
        parts.push(format!("failed by {}", crate::words::and_list(&named)));
    }
    if let Some(why) = &r.set_aside {
        parts.push(format!("set aside: {}", cell(why)));
    }
    parts.join(" · ")
}

/// A combination under test: each task as it stands. A task that ended
/// shows no outcome until the combination's results are read, so no pull
/// request is told of a failure before it is attributed (20).
fn live(tasks: &[TaskProgress]) -> String {
    let mut s = String::from("\n|  | Check | Time | State |\n|---|---|---|---|\n");
    let mut ordered: Vec<&TaskProgress> = tasks.iter().collect();
    ordered.sort_by_key(|t| (rank(t.state), t.target.clone()));
    for t in ordered {
        let (icon, word) = match t.state {
            TaskState::Running => ("🔄", "Running"),
            TaskState::Waiting => ("⏳", "Waiting"),
            TaskState::Passed | TaskState::Failed => ("⬜", "Ended"),
            TaskState::Replayed => ("🟪", "Replayed"),
        };
        let took = match (&t.started, &t.ended) {
            (Some(a), Some(b)) => between(a, b).map(time).unwrap_or_default(),
            _ => String::new(),
        };
        s.push_str(&format!("| {icon} | `{}` | {took} | {word} |\n", t.target));
    }
    s
}

fn rank(s: TaskState) -> u8 {
    match s {
        TaskState::Running => 0,
        TaskState::Waiting => 1,
        _ => 2,
    }
}

fn between(a: &str, b: &str) -> Option<u64> {
    let a = chrono::DateTime::parse_from_rfc3339(a).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(b).ok()?;
    u64::try_from((b - a).num_milliseconds()).ok()
}

fn executor(e: &Enforced) -> String {
    let mut parts = vec![e.kind.clone()];
    parts.push(if e.confined {
        "confined".into()
    } else {
        "not confined".into()
    });
    if let Some(p) = e.processors {
        parts.push(format!("{p} processors"));
    }
    if let Some(m) = e.memory {
        parts.push(format!("{} memory", bytes(m)));
    }
    if let Some(n) = &e.network {
        parts.push(format!("network {n:?}").to_lowercase());
    }
    parts.join(", ")
}

/// What failed checks printed, as the policy asks, scrubbed.
fn outputs(rows: &[Row], cx: &Context) -> String {
    let Some(n) = cx.log_lines else {
        return String::new();
    };
    let mut s = String::new();
    for r in rows.iter().filter(|r| r.result != RowResult::Passed) {
        let Some(text) = r.log.as_deref().and_then(|at| (cx.read_log)(at)) else {
            continue;
        };
        let tail = cx.scrub.scrub(last_lines(&text, n));
        s.push_str(&details(
            &format!("Output of {}, its last {n} lines", check(r)),
            &format!("```\n{}\n```", tail.trim_end().replace("```", "'''")),
        ));
    }
    s
}

/// Where each failed check's output is kept, when it is not posted.
fn kept(rows: &[Row], cx: &Context) -> String {
    if cx.log_lines.is_some() {
        return String::new();
    }
    let lines: Vec<String> = rows
        .iter()
        .filter(|r| r.result != RowResult::Passed)
        .filter_map(|r| Some(format!("- {}: `{}`", check(r), r.log.as_deref()?)))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\nWhat the failed checks printed is kept on the host that ran them:\n\n{}\n",
        lines.join("\n")
    )
}

/// `8 GiB`, `512 MiB`.
fn bytes(n: u64) -> String {
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if n >= GIB && n.is_multiple_of(GIB) {
        format!("{} GiB", n / GIB)
    } else {
        format!("{} MiB", n / MIB)
    }
}

fn details(summary: &str, body: &str) -> String {
    format!("\n<details><summary>{summary}</summary>\n\n{body}\n\n</details>\n")
}

/// Text made safe for a table cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

/// `2m 3s`, `341ms`, `1h 2m`.
pub fn time(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    let s = ms / 1_000;
    match s {
        0..=59 => format!("{s}s"),
        60..=3_599 if s.is_multiple_of(60) => format!("{}m", s / 60),
        60..=3_599 => format!("{}m {}s", s / 60, s % 60),
        _ if (s % 3_600) < 60 => format!("{}h", s / 3_600),
        _ => format!("{}h {}m", s / 3_600, (s % 3_600) / 60),
    }
}

/// The check of one row, `sluice / <check> (<system>)`, as posted.
pub fn row_check(r: &Row, cx: &Context) -> (String, Posting) {
    let status = match r.result {
        RowResult::Passed => Status::Success,
        RowResult::SetAside => Status::Queued,
        _ => Status::Failure,
    };
    let mut title = result_word(r).to_lowercase();
    let i = info(r, false);
    if !i.is_empty() {
        title.push_str(&format!(" · {i}"));
    }
    let mut summary = format!(
        "|  | Check | Time | Result | Info |\n|---|---|---|---|---|\n{}",
        row_line(r)
    );
    if let Some(k) = &r.key {
        summary.push_str(&format!("\nKey `{k}`.\n"));
    }
    let text = Some(outputs(std::slice::from_ref(r), cx)).filter(|t| !t.is_empty());
    (
        check_name(&r.task, r.system.as_deref()),
        Posting {
            status,
            title,
            summary,
            text,
        },
    )
}

/// A check per check while its combination is still under test, or after
/// it was formed again: no outcome yet.
pub fn pending_check(status: Status, word: &str) -> Posting {
    Posting {
        status,
        title: word.into(),
        summary: String::new(),
        text: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sluice_core::verdict_view::{Contradicted, RunShown};
    use sluice_seams::{CoveringVerdict, Rev, RunDetail};

    fn row(task: &str, result: RowResult) -> Row {
        Row {
            task: task.into(),
            system: None,
            key: Some(format!("{task}-key")),
            result,
            replayed: false,
            host: Some("h1".into()),
            signer: None,
            executor: Some(Enforced {
                kind: "sandbox".into(),
                confined: true,
                processors: Some(4),
                memory: Some(8 << 30),
                network: None,
                docker: false,
                grant: None,
            }),
            took_ms: Some(18_000),
            attempts: Some(1),
            flaky: None,
            not_run: None,
            failed_by: Vec::new(),
            set_aside: None,
            log: None,
        }
    }

    fn view(standing: Standing, rows: Vec<Row>) -> VerdictView {
        VerdictView {
            branch: "main".into(),
            id: 12,
            standing,
            combination: Some(Rev::new("a1b2c3d4e5f6")),
            on: Some(Rev::new("9f8e7d6c5b4a")),
            submissions: vec![10, 12],
            runs: vec![RunShown {
                system: None,
                detail: RunDetail {
                    took_ms: Some(252_000),
                    estimate_ms: Some(580_000),
                    changed: vec!["app/src/lib.rs".into()],
                },
            }],
            rows,
            tasks: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn with<T>(log_lines: Option<usize>, f: impl FnOnce(&Context) -> T) -> T {
        let names = BTreeMap::from([(10, "#10".to_string()), (12, "#12".to_string())]);
        let scrub = Scrub::new(std::iter::empty());
        let read = |at: &str| {
            (at == "/kept/app-test.log").then(|| {
                "line 1\nline 2\nGITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123\nassertion failed\n"
                    .to_string()
            })
        };
        let cx = Context {
            names: &names,
            repo_url: "https://github.com/o/r".into(),
            log_lines,
            read_log: &read,
            scrub: &scrub,
        };
        f(&cx)
    }

    fn blamed() -> VerdictView {
        let mut failed = row("app:test", RowResult::Failed);
        failed.took_ms = Some(123_000);
        failed.attempts = Some(2);
        failed.log = Some("/kept/app-test.log".into());
        let mut replayed = row("core:test", RowResult::Passed);
        replayed.replayed = true;
        replayed.host = None;
        replayed.executor = None;
        replayed.took_ms = None;
        let mut flaky = row("app:e2e", RowResult::Flaky);
        flaky.flaky = Some(Contradicted {
            outcome: sluice_seams::Outcome::Pass,
            at: "2026-10-19T10:00:00Z".into(),
            combination: Rev::new("c0"),
            worktree: None,
        });
        let mut could_not = row("site:check", RowResult::NotRun);
        could_not.not_run =
            Some("exited 77, which [tests] not_run names: it could not run here, 3 runs".into());
        could_not.key = None;
        view(
            Standing::Blamed { timed_out: false },
            vec![
                failed,
                flaky,
                could_not,
                row("app:lint", RowResult::Passed),
                replayed,
            ],
        )
    }

    #[test]
    fn a_run_that_saved_nothing_claims_no_saving() {
        let mut v = blamed();
        v.runs[0].detail.took_ms = Some(56_000);
        v.runs[0].detail.estimate_ms = Some(47_000);
        let s = with(None, |cx| report(&v, cx));
        assert!(
            s.contains("Total time: 56s | Comparison time: 47s\n"),
            "{s}"
        );
        assert!(!s.contains("Estimated savings"), "{s}");
    }

    #[test]
    fn a_blamed_pull_request() {
        let s = with(None, |cx| report(&blamed(), cx));
        assert!(
            s.starts_with(
                "<!-- sluice-report: main -->\n## sluice: failed on main, #12 is to blame\n"
            ),
            "{s}"
        );
        assert!(s.contains("Tested `a1b2c3d` = main [`9f8e7d6`](https://github.com/o/r/commit/9f8e7d6c5b4a) + #10 + #12 · 3 ran, 1 replayed"), "{s}");
        assert!(s.contains("Total time: 4m 12s | Comparison time: 9m 40s | Estimated savings: 5m 28s (56% faster)"), "{s}");
        assert!(
            s.contains(
                "| 🟥 | `app:test` | 2m 3s | Failed | sandbox on h1 · 2 attempts · **slow** |"
            ),
            "{s}"
        );
        assert!(s.contains("| 🟨 | `app:e2e` | 18s | Flaky | sandbox on h1 · passed on the same inputs at 2026-10-19T10:00:00Z |"), "{s}");
        assert!(
            s.contains("| ⬛ | `site:check` | 18s | Could not run |"),
            "{s}"
        );
        assert!(
            s.contains("| 🟪 | `core:test` |  | Replayed | from a pass the gate trusts |"),
            "{s}"
        );
        assert!(
            s.contains(
                "<summary>Executor</summary>\n\nsandbox, confined, 4 processors, 8 GiB memory"
            ),
            "{s}"
        );
        assert!(s.contains("- `app/src/lib.rs`"), "{s}");
        assert!(s.contains("- `app:test`: `/kept/app-test.log`"), "{s}");
        assert!(
            !s.contains("assertion failed"),
            "no output unless asked: {s}"
        );
        // Rows keep the order they come in: failed first.
        assert!(s.find("app:test").unwrap() < s.find("app:lint").unwrap());
        // Deterministic: drawn twice, the same.
        assert_eq!(s, with(None, |cx| report(&blamed(), cx)));
    }

    /// 149; the verdict-report spec's "The pull request's check": a check
    /// failed by a task it depends on is drawn failed, saying which task,
    /// with no key, and the task failed with its key and its log.
    #[test]
    fn a_check_failed_by_a_dependency() {
        let mut dep = row("api:integration", RowResult::Failed);
        dep.log = Some("/kept/api-integration.log".into());
        let mut check = row("root:integration", RowResult::Failed);
        check.key = None;
        check.host = None;
        check.executor = None;
        check.took_ms = None;
        check.attempts = None;
        check.failed_by = vec!["api:integration".into()];
        let v = view(Standing::Blamed { timed_out: false }, vec![dep, check]);
        let s = with(None, |cx| report(&v, cx));
        assert!(
            s.contains("| 🟥 | `root:integration` |  | Failed | failed by `api:integration` |"),
            "{s}"
        );
        assert!(
            s.contains("| 🟥 | `api:integration` | 18s | Failed | sandbox on h1 |"),
            "{s}"
        );
        assert!(
            s.contains("- `api:integration`: `/kept/api-integration.log`"),
            "{s}"
        );
        assert!(!s.contains("- `root:integration`:"), "{s}");

        let (name, posting) = with(None, |cx| row_check(&v.rows[1], cx));
        assert_eq!(name, check_name("root:integration", None));
        assert_eq!(posting.status, Status::Failure);
        assert_eq!(posting.title, "failed · failed by api:integration");
        assert!(!posting.summary.contains("Key "), "{}", posting.summary);
        let (_, posting) = with(None, |cx| row_check(&v.rows[0], cx));
        assert!(
            posting.summary.contains("\nKey `api:integration-key`.\n"),
            "{}",
            posting.summary
        );

        // Two tasks: each named, the last joined with `and`.
        let mut two = v.rows[1].clone();
        two.failed_by.push("ui:integration".into());
        assert_eq!(
            info(&two, true),
            "failed by `api:integration` and `ui:integration`"
        );
    }

    #[test]
    fn output_only_as_asked_and_scrubbed() {
        let s = with(Some(2), |cx| report(&blamed(), cx));
        assert!(
            s.contains("<summary>Output of `app:test`, its last 2 lines</summary>"),
            "{s}"
        );
        assert!(s.contains("[redacted]\nassertion failed"), "{s}");
        assert!(!s.contains("ghp_") && !s.contains("line 1"), "{s}");
        assert!(!s.contains("is kept on the host"), "{s}");
    }

    #[test]
    fn landed_testing_and_formed_again() {
        let mut v = view(
            Standing::Landed {
                landed_as: Rev::new("0cef58d1234"),
                beside: false,
                covered_by: None,
            },
            vec![row("app:lint", RowResult::Passed)],
        );
        let s = with(None, |cx| report(&v, cx));
        assert!(s.contains("## sluice: landed on main as 0cef58d\n"), "{s}");

        // 140: landed beside the queue: no check ran, no row is shown,
        // and the report names the result that covers it, its revision
        // linked to its commit (23; record 0083).
        let at = "e25fea4c0ffee0000000000000000000000000ab";
        let mut beside = view(
            Standing::Landed {
                landed_as: Rev::new("0cef58d1234"),
                beside: true,
                covered_by: Some(CoveringVerdict {
                    at: Rev::new(at),
                    result: "passed".into(),
                    ran: 141,
                    replayed: 0,
                }),
            },
            Vec::new(),
        );
        assert!(has_report(&beside));
        let s = with(None, |cx| report(&beside, cx));
        assert_eq!(
            s,
            format!(
                "<!-- sluice-report: main -->\n## sluice: landed on main as 0cef58d with no \
                 check run: no required check reads its changes\n\nCovered by the result at \
                 [`e25fea4`](https://github.com/o/r/commit/{at}), passed (141 run, 0 \
                 replayed).\n"
            )
        );
        // One recorded before the result that covers it was kept has no
        // report: its check's summary alone says it landed beside.
        beside.standing = Standing::Landed {
            landed_as: Rev::new("0cef58d1234"),
            beside: true,
            covered_by: None,
        };
        assert!(!has_report(&beside));

        v.standing = Standing::Testing;
        v.rows.clear();
        v.tasks = vec![
            TaskProgress {
                target: "app:test".into(),
                state: TaskState::Failed,
                started: Some("2026-10-20T10:00:00Z".into()),
                ended: Some("2026-10-20T10:02:03Z".into()),
                exit: Some(1),
            },
            TaskProgress {
                target: "app:lint".into(),
                state: TaskState::Running,
                started: Some("2026-10-20T10:00:00Z".into()),
                ended: None,
                exit: None,
            },
        ];
        let s = with(None, |cx| report(&v, cx));
        assert!(s.contains("| 🔄 | `app:lint` |  | Running |"), "{s}");
        // An ended task tells no outcome before its combination's are read.
        assert!(s.contains("| ⬜ | `app:test` | 2m 3s | Ended |"), "{s}");
        assert!(!s.contains("Failed"), "{s}");

        let formed = VerdictView {
            standing: Standing::FormedAgain,
            combination: None,
            on: None,
            submissions: Vec::new(),
            runs: Vec::new(),
            rows: Vec::new(),
            ..v
        };
        let s = with(None, |cx| report(&formed, cx));
        assert!(s.contains("## sluice: formed again on main"), "{s}");
        assert!(!s.contains("#10") && !s.contains("app:"), "{s}");
    }

    #[test]
    fn many_checks_fold_and_a_huge_report_is_cut() {
        let rows: Vec<Row> = (0..45)
            .map(|i| row(&format!("p{i:02}:test"), RowResult::Passed))
            .collect();
        let v = view(Standing::Passed, rows);
        let s = with(None, |cx| report(&v, cx));
        assert!(
            s.contains("<summary>Every check (25 more)</summary>"),
            "{s}"
        );
        assert_eq!(s.matches("| 🟩 |").count(), 45);

        let mut huge = blamed();
        huge.rows[0].log = Some("/kept/app-test.log".into());
        huge.runs[0].detail.changed = (0..5_000).map(|i| format!("dir/file-{i:05}.rs")).collect();
        let rows: Vec<Row> = (0..2_000)
            .map(|i| {
                row(
                    &format!("project-{i:04}:a-rather-long-task-name"),
                    RowResult::Passed,
                )
            })
            .collect();
        huge.rows.extend(rows);
        let s = with(Some(40), |cx| report(&huge, cx));
        assert!(s.len() <= MOST, "{}", s.len());
        assert!(s.contains("more checks, every one shown by `sluice status`"));
    }

    #[test]
    fn a_rows_own_check() {
        let mut r = row("cli:build", RowResult::Passed);
        r.system = Some("linux-x64".into());
        r.signer = Some("box".into());
        let (name, p) = with(None, |cx| row_check(&r, cx));
        assert_eq!(name, "sluice / cli:build (linux-x64)");
        assert_eq!(p.status, Status::Success);
        assert_eq!(p.title, "passed · sandbox on h1 · signed by box");
        r.took_ms = Some(SLOW_MS + 1);
        let (_, p) = with(None, |cx| row_check(&r, cx));
        assert_eq!(p.title, "passed · sandbox on h1 · signed by box · slow");
        assert!(p.summary.contains("· **slow** |"), "{}", p.summary);
        let f = row("app:test", RowResult::Failed);
        assert_eq!(with(None, |cx| row_check(&f, cx)).1.status, Status::Failure);
    }

    #[test]
    fn times() {
        assert_eq!(time(341), "341ms");
        assert_eq!(time(18_000), "18s");
        assert_eq!(time(123_000), "2m 3s");
        assert_eq!(time(120_000), "2m");
        assert_eq!(time(3_720_000), "1h 2m");
    }
}
