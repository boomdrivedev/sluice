//! How sluice words a failed check, the same in a waiting `submit` and on
//! a pull request (ADR 0044), and a required check that can never pass
//! (ADR 0050). An outcome's notice is the core's, `sluice_core::notice`,
//! since the round records it as its delivery's message (design D2).

use sluice_core::notice::NO_CHECK_READS;
use sluice_core::records::{BesideWhy, FlakyKey, Recorded};
use sluice_core::KeptOut;
use sluice_seams::{Outcome, Rev, Submitter};

/// `passed` or `failed`.
pub fn outcome(o: Outcome) -> &'static str {
    match o {
        Outcome::Pass => "passed",
        Outcome::Fail => "failed",
    }
}

/// Where a recorded run ran: a session's worktree, or a combination.
pub fn recorded_place(r: &Recorded) -> String {
    match &r.worktree {
        Some(w) => format!("in {w}"),
        None => format!("on {}", r.combination.short()),
    }
}

/// 19: a key read as flaky, against the result it contradicts.
pub fn flaky(task: &str, k: &FlakyKey) -> String {
    format!(
        "{task} {} in this run and {} on the same inputs at {} {}",
        outcome(k.here),
        outcome(k.other.outcome),
        k.other.at,
        recorded_place(&k.other)
    )
}

/// 19: how a head whose key reads flaky lands: once a real fix changes
/// what the test reads, never a change to what it reads only to have it
/// tested again, and no step of the operator's. A fail the host caused is
/// a fault of sluice, which sluice fixes so such a run records no outcome
/// (the change `sluice-host-makes-each-change`, design D4).
pub fn flaky_way_on(tasks: &[&str]) -> String {
    format!(
        "the same head lands once a real fix changes what {} {}; a fail the host caused, not the test, is a fault of sluice",
        tasks.join(", "),
        if tasks.len() == 1 { "reads" } else { "read" }
    )
}

/// Time-limit-1 66: the tests a run's time limit ended, which have no
/// result.
pub fn cut_off(tasks: &[&str]) -> String {
    format!(
        "timed out: {} {} running when the time limit ended the run",
        tasks.join(", "),
        if tasks.len() == 1 { "was" } else { "were" }
    )
}

/// Who moves a submission on: the agent working in the worktree it came
/// from, named by that worktree's branch, since the session that built
/// the work submits it (statement preamble; section 2, actors).
pub fn agent_in(branch: &str) -> String {
    format!("the agent working in {branch}")
}

/// Who moves a submission on, as its way on names them (D10).
#[derive(Clone, Copy, Debug)]
pub enum Mover<'a> {
    /// The agent working in the worktree it came from, named by that
    /// worktree's branch, a pull request's the one it was opened from.
    Agent(&'a str),
    /// For `branch` submitted by name to `target`, in which no agent
    /// works: whoever submits it by name, once a fix lands on the branch
    /// (124; record 0090; flume 203).
    ByName { branch: &'a str, target: &'a str },
}

impl<'a> Mover<'a> {
    /// Who moves on the submission to `target` that `from` made.
    pub fn of(from: &'a Submitter, target: &'a str) -> Mover<'a> {
        if from.by_name() {
            Mover::ByName {
                branch: &from.worktree,
                target,
            }
        } else {
            Mover::Agent(from.head.as_deref().unwrap_or(&from.worktree))
        }
    }
}

/// 124, D10: the submit that moves `branch`'s submission to `target` on,
/// once a fix lands on the branch.
fn submit_by_name(branch: &str, target: &str) -> String {
    format!("sluice submit {target} --head {branch}")
}

/// Time-limit-1 66: the way on after a run its time limit ended, for
/// `mover`. Nothing was recorded for a test it cut off, so the same head
/// is tested afresh, a pull request's too (120).
pub fn timed_out_way_on(mover: Mover<'_>, tasks: &[&str]) -> String {
    let (recorded, which) = match tasks {
        [] => (String::new(), "a test".to_string()),
        [one] => (format!("nothing was recorded for {one}: "), one.to_string()),
        _ => (
            format!("nothing was recorded for {}: ", tasks.join(", ")),
            "one".to_string(),
        ),
    };
    match mover {
        Mover::Agent(worktree) => format!(
            "{recorded}if the host was slow, {} submits the same head again; \
             if {which} never ends, it fixes it there and submits again",
            agent_in(worktree)
        ),
        Mover::ByName { branch, target } => format!(
            "{recorded}if the host was slow, {} submits the same head again; \
             if {which} never ends, a fix lands on {branch} first",
            submit_by_name(branch, target)
        ),
    }
}

/// The way on after a check failed on this submission, for `mover`.
pub fn fix_way_on(mover: Mover<'_>) -> String {
    match mover {
        Mover::Agent(worktree) => {
            format!("{} fixes it there and submits again", agent_in(worktree))
        }
        Mover::ByName { branch, target } => format!(
            "once a fix lands on {branch}, {} submits it again",
            submit_by_name(branch, target)
        ),
    }
}

/// Could-not-run-1 115: the way on after the host could not run `tasks`,
/// for `mover`.
pub fn could_not_run_way_on(mover: Mover<'_>, tasks: &[&str]) -> String {
    let again = match mover {
        Mover::Agent(worktree) => format!("{} submits again", agent_in(worktree)),
        Mover::ByName { branch, target } => {
            format!("{} submits it again", submit_by_name(branch, target))
        }
    };
    format!(
        "the host could not run {}: once it can, {again}",
        tasks.join(", ")
    )
}

/// The way on after a conflict with what is ahead of it on `onto`, for
/// `mover`. A fix to a branch submitted by name merges `onto` at `at`, the
/// revision it was tested on, or the branch's where it was not tested
/// (D10); an agent merges or rebases in its own worktree, and `at` is not
/// read for it.
pub fn conflict_way_on(mover: Mover<'_>, onto: &str, at: Option<&Rev>) -> String {
    match mover {
        Mover::Agent(worktree) => format!(
            "{} merges or rebases onto {onto} there and submits again",
            agent_in(worktree)
        ),
        Mover::ByName { branch, target } => {
            let at = at.map_or(String::new(), |r| format!(" at {}", r.short()));
            format!(
                "once a fix that merges {onto}{at} lands on {branch}, {} submits it again",
                submit_by_name(branch, target)
            )
        }
    }
}

/// 140: a waiting submit's last line for a submission landed beside the
/// queue as `landed_as`.
pub fn landed_beside(landed_as: &Rev) -> String {
    format!(
        "landed as {} beside the queue: {NO_CHECK_READS}",
        landed_as.short()
    )
}

/// 122: a waiting submit's last line for a submission whose work
/// `branch` holds by a landing not its own, as `landed_as`.
pub fn landed_elsewhere(branch: &str, landed_as: &Rev) -> String {
    format!(
        "landed as {}: {branch} holds its work, by a landing not its own",
        landed_as.short()
    )
}

/// 122, record 0100 D3: a wait's last line for its own submission, whose
/// work `branch` holds as `landed_as` by the landing of `by`, what carried
/// that work and the wait followed: `#<b>` or `pull request #<n>`.
pub fn landed_carried(branch: &str, landed_as: &Rev, by: &str) -> String {
    format!(
        "landed as {}: {branch} holds its work, by {by}'s landing",
        landed_as.short()
    )
}

/// 67, record 0100 D3: a wait's last line for its own submission `id`,
/// which `left` the queue (`withdrawn by <who>`), once `by`, what carried
/// its work and the wait followed, stopped short as `how` says (`failed`,
/// `is in conflict`, `was withdrawn by <who>`, `was closed without
/// landing`, `was sent back`).
pub fn carrier_stopped(id: u64, left: &str, by: &str, how: &str) -> String {
    format!("#{id} {left}: {by}, which carried its work, {how}")
}

/// 140: what a submission that takes the queue though no check reads its
/// changes at the branch's revision waits for: entry `behind`, whose
/// checks would read it or whose work it conflicts with.
pub fn waits_beside(behind: u64, why: BesideWhy) -> String {
    match why {
        BesideWhy::Read => format!("waits behind #{behind}: its checks read this change"),
        BesideWhy::Conflict => format!("conflicts with #{behind}'s work"),
    }
}

/// 51: the line `sluice inbox` ends an outcome with, naming the key to
/// confirm it by; an expired one's opening with when it expired, still
/// owed (weir 104; design D8).
pub fn confirm_line(o: &sluice_core::OwedOutcome) -> String {
    let confirm = format!(
        "Confirm it once you have acted on it: sluice confirm {}",
        o.key
    );
    match o.owed.expired {
        true => format!(
            "Expired at {}, still owed. {confirm}",
            o.owed.delivery.expires_at()
        ),
        false => confirm,
    }
}

/// 51: `sluice confirm`'s line for a key it confirmed: the key, and the
/// outcome it names on its branch (design D7).
pub fn confirmed(key: &str, id: u64, branch: &str) -> String {
    let what = match key.rsplit_once(':').map(|(_, w)| w) {
        Some("landed") => format!("#{id} landed on {branch}"),
        Some("failed") => format!("#{id} failed on {branch}"),
        Some("conflict") => format!("#{id} conflicts on {branch}"),
        _ => format!("#{id} on {branch}"),
    };
    format!("confirmed {key} ({what})")
}

/// Could-not-run-1 115: a check that could not run, and why.
pub fn could_not_run(task: &str, why: &str) -> String {
    format!("{task} could not run: {why}")
}

/// 139 (design D8 of the change
/// `a-check-its-executor-refused-has-no-outcome`): a run whose executor
/// could not read what its sandbox refused it, which was read as refused
/// nothing, with the words of what failed.
pub fn refusals_unread(why: &str) -> String {
    format!("sluice could not read what its sandbox refused the run: {why}")
}

/// C.6, 20 (design D5 of the change
/// `a-signed-run-without-a-sandbox-proves-its-passes`): a check failed
/// under a key the combination's tree does not bear out, with the check
/// profile's words for why.
pub fn not_borne_out(task: &str, why: &str) -> String {
    format!("{task}: the combination's tree does not bear its key out: {why}")
}

/// 149: a required check its runner skipped for the tasks `by` it depends
/// on, which ran and failed; the tasks joined with `, ` and the last with
/// ` and `.
pub fn failed_by(check: &str, by: &[String]) -> String {
    let what = if by.len() > 1 {
        "tasks it depends on"
    } else {
        "a task it depends on"
    };
    format!("{check} failed by {}, {what}", and_list(by))
}

/// `items` joined with `, `, the last with ` and `.
pub fn and_list(items: &[String]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.concat(),
    }
}

/// ADR 0050: a submission refused because `branch` requires checks the
/// task runner in the submitting worktree keeps out of CI: each, through
/// the tag that names it, with its line of the policy, and the way on for
/// the agent working in `worktree`.
pub fn kept_out_refusal(branch: &str, worktree: &str, kept: &[KeptOut]) -> String {
    let mut s = String::from("refused: ");
    for k in kept {
        let through = if k.entry == k.task {
            String::new()
        } else {
            format!(" through {}", k.entry)
        };
        s.push_str(&format!(
            "{branch} requires {}{through} (.config/sluice.toml line {}), which the task runner \
             in this worktree keeps out of CI, so it is never run and {branch} cannot pass while \
             it is required\n",
            k.task, k.line
        ));
    }
    let which = if kept.len() == 1 { "it" } else { "each" };
    s.push_str(&format!(
        "{} lets {which} run in CI there, or drops it from {branch}'s policy in the same \
         submission that lets it run, and submits again",
        agent_in(worktree)
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 139, design D8 (the run-outcome spec's "The words").
    #[test]
    fn an_unread_report_is_said() {
        assert_eq!(
            refusals_unread("log: exited 1"),
            "sluice could not read what its sandbox refused the run: log: exited 1"
        );
    }

    /// The pass-proof spec's "The words" (design D5 of the change
    /// `a-signed-run-without-a-sandbox-proves-its-passes`).
    #[test]
    fn a_key_not_borne_out_is_said_with_why() {
        assert_eq!(
            not_borne_out("a:test", "a:test: a/x is not in the tree"),
            "a:test: the combination's tree does not bear its key out: \
             a:test: a/x is not in the tree"
        );
    }

    fn kept(task: &str, line: usize) -> KeptOut {
        KeptOut {
            task: task.into(),
            entry: task.into(),
            line,
        }
    }

    /// 120, pull-request-submit: the way on after a time-out, a blame, a
    /// check the host could not run or a conflict is a submit, which
    /// sluice takes, a pull request's included; none names a draft.
    #[test]
    fn the_ways_on_name_a_submit_and_no_draft() {
        let a = Mover::Agent("unit/a");
        let at = Rev::new("abc1234def");
        let ways = [
            timed_out_way_on(a, &[]),
            timed_out_way_on(a, &["cli:test"]),
            timed_out_way_on(a, &["cli:test", "cli:tools"]),
            fix_way_on(a),
            could_not_run_way_on(a, &["cli:tools"]),
            conflict_way_on(a, "main", Some(&at)),
        ];
        for w in &ways {
            assert!(w.contains("submit"), "{w}");
            assert!(
                !w.contains("draft") && !w.contains("ready for review"),
                "{w}"
            );
            assert!(!w.contains("your worktree"), "{w}");
        }
        assert_eq!(
            fix_way_on(a),
            "the agent working in unit/a fixes it there and submits again"
        );
        assert_eq!(
            could_not_run_way_on(a, &["cli:tools"]),
            "the host could not run cli:tools: once it can, the agent working in unit/a submits \
             again"
        );
        assert_eq!(
            conflict_way_on(a, "main", Some(&at)),
            "the agent working in unit/a merges or rebases onto main there and submits again"
        );
    }

    fn from(worktree: &str, head: Option<&str>) -> Submitter {
        Submitter {
            agent: "a".into(),
            worktree: worktree.into(),
            path: None,
            head: head.map(str::to_string),
            host: None,
            session: None,
        }
    }

    /// 124, D10: the way on of a branch submitted by name is its caller's:
    /// a fix landed on the branch, then the submit by name again; for a
    /// conflict, the fix merges the target at the revision named first. No
    /// agent works in a branch sluice lands on, so none is named; a
    /// worktree's own submission and a pull request's still name theirs.
    #[test]
    fn the_way_on_by_name_is_its_callers() {
        let named = from("bolt/x", Some("bolt/x"));
        let by_name = Mover::of(&named, "main");
        let at = Rev::new("abc1234def");
        assert_eq!(
            fix_way_on(by_name),
            "once a fix lands on bolt/x, sluice submit main --head bolt/x submits it again"
        );
        assert_eq!(
            conflict_way_on(by_name, "main", Some(&at)),
            "once a fix that merges main at abc1234 lands on bolt/x, sluice submit main --head \
             bolt/x submits it again"
        );
        assert_eq!(
            timed_out_way_on(by_name, &["cli:test"]),
            "nothing was recorded for cli:test: if the host was slow, sluice submit main --head \
             bolt/x submits the same head again; if cli:test never ends, a fix lands on bolt/x \
             first"
        );
        assert_eq!(
            could_not_run_way_on(by_name, &["cli:tools"]),
            "the host could not run cli:tools: once it can, sluice submit main --head bolt/x \
             submits it again"
        );
        for w in [
            fix_way_on(by_name),
            conflict_way_on(by_name, "main", Some(&at)),
            conflict_way_on(by_name, "main", None),
            timed_out_way_on(by_name, &[]),
            could_not_run_way_on(by_name, &["cli:tools"]),
        ] {
            assert!(!w.contains("agent"), "{w}");
            assert!(w.contains("sluice submit main --head bolt/x"), "{w}");
        }
        let own = from("unit/a", None);
        assert!(matches!(Mover::of(&own, "main"), Mover::Agent("unit/a")));
        let request = from("pull/9", Some("feature/a"));
        assert!(matches!(
            Mover::of(&request, "main"),
            Mover::Agent("feature/a")
        ));
    }

    #[test]
    fn a_time_limit_names_the_agent_in_the_worktree() {
        assert_eq!(
            timed_out_way_on(Mover::Agent("unit/a"), &["tests:t-cards"]),
            "nothing was recorded for tests:t-cards: if the host was slow, the agent working \
             in unit/a submits the same head again; if tests:t-cards never ends, it fixes it \
             there and submits again"
        );
        assert_eq!(
            timed_out_way_on(Mover::Agent("unit/a"), &["tests:a", "tests:b"]),
            "nothing was recorded for tests:a, tests:b: if the host was slow, the agent \
             working in unit/a submits the same head again; if one never ends, it fixes it \
             there and submits again"
        );
    }

    /// 149: a check failed by the tasks it depends on names each, the
    /// last joined with ` and `, as design D8 words it.
    #[test]
    fn a_check_failed_by_its_dependencies_names_each() {
        assert_eq!(
            failed_by("root:integration", &["api:integration".into()]),
            "root:integration failed by api:integration, a task it depends on"
        );
        assert_eq!(
            failed_by(
                "root:integration",
                &[
                    "api:integration".into(),
                    "ui:integration".into(),
                    "web:integration".into()
                ]
            ),
            "root:integration failed by api:integration, ui:integration and web:integration, \
             tasks it depends on"
        );
    }

    /// 140: a landing beside the queue, and what an entry that takes the
    /// queue instead waits behind, in the record's words.
    #[test]
    fn beside_the_queue_in_words() {
        assert_eq!(
            landed_beside(&Rev::new("abc1234def")),
            "landed as abc1234 beside the queue: no required check reads its changes"
        );
        assert_eq!(
            waits_beside(7, BesideWhy::Read),
            "waits behind #7: its checks read this change"
        );
        assert_eq!(
            waits_beside(7, BesideWhy::Conflict),
            "conflicts with #7's work"
        );
    }

    #[test]
    fn a_check_kept_out_of_ci_names_the_agent_in_the_worktree() {
        let one = kept_out_refusal("main", "unit/a", &[kept("cli:e2e", 7)]);
        assert!(
            one.ends_with(
                "\nthe agent working in unit/a lets it run in CI there, or drops it from \
                 main's policy in the same submission that lets it run, and submits again"
            ),
            "{one}"
        );
        let two = kept_out_refusal("main", "unit/a", &[kept("cli:e2e", 7), kept("site:e2e", 8)]);
        assert!(
            two.ends_with(
                "\nthe agent working in unit/a lets each run in CI there, or drops it from \
                 main's policy in the same submission that lets it run, and submits again"
            ),
            "{two}"
        );
        assert!(!two.contains("your worktree"), "{two}");
    }
}
