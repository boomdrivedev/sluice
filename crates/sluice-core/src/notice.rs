//! 51: an outcome's notice, one line, which the round records as its
//! delivery's message and pushes into the submitting session, and which
//! `sluice inbox` prints (record 0102; design D2).

use sluice_seams::Decided;

/// 140: why a submission landed beside the queue with nothing run.
pub const NO_CHECK_READS: &str = "no required check reads its changes";

/// 51: an outcome's notice, one line, the same whether the round pushes it
/// into the submitting session or the session collects it by `sluice
/// inbox`: the submission, its branch and its outcome, and for a failure
/// or a conflict the command that shows the rest. It opens `[sluice]`, so
/// it is never taken for the user's typing.
pub fn notice(id: u64, branch: &str, decided: &Decided) -> String {
    match decided {
        Decided::Landed {
            landed_as,
            beside: false,
            ..
        } => {
            format!(
                "[sluice] #{id} landed on {branch} as {}.",
                landed_as.short()
            )
        }
        Decided::Landed {
            landed_as,
            beside: true,
            ..
        } => {
            format!(
                "[sluice] #{id} landed on {branch} as {} beside the queue: {NO_CHECK_READS}.",
                landed_as.short()
            )
        }
        Decided::Failed {
            failed, timed_out, ..
        } => {
            let what = if *timed_out {
                ": timed out".to_string()
            } else if failed.is_empty() {
                String::new()
            } else {
                format!(": {}", failed.join(", "))
            };
            format!("[sluice] #{id} failed on {branch}{what}. sluice wait {id} shows why.")
        }
        Decided::Conflict { paths, with } => {
            let mut text = format!("[sluice] #{id} conflicts on {branch}");
            if !with.is_empty() {
                let ids: Vec<String> = with.iter().map(|c| format!("#{}", c.id)).collect();
                text.push_str(&format!(" with {}", ids.join(", ")));
            }
            if !paths.is_empty() {
                text.push_str(&format!(" in {}", paths.join(", ")));
            }
            text.push_str(&format!(". sluice wait {id} shows it."));
            text
        }
        // 152, record 0124 (design D10).
        Decided::Refused { words } => format!(
            "[sluice] #{id}'s landing on {branch} was refused by the forge and is held: {words}. \
             Withdraw it or submit again from its place, or the operator pushes it again once \
             the forge takes it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use sluice_seams::{Contested, Rev, Submitter};

    use super::*;

    /// 51: each outcome is one line naming the submission, its branch and
    /// its outcome, a failure and a conflict the command showing the rest.
    #[test]
    fn an_outcomes_notice_is_one_line() {
        let landed = Decided::Landed {
            landed_as: Rev::new("abc1234def"),
            beside: false,
            covered_by: None,
        };
        assert_eq!(
            notice(12, "main", &landed),
            "[sluice] #12 landed on main as abc1234."
        );
        // 140: landed beside the queue, with no check run.
        let beside = Decided::Landed {
            landed_as: Rev::new("abc1234def"),
            beside: true,
            covered_by: None,
        };
        assert_eq!(
            notice(12, "main", &beside),
            "[sluice] #12 landed on main as abc1234 beside the queue: no required check \
             reads its changes."
        );
        let failed = |timed_out| Decided::Failed {
            failed: vec!["cli:test".into(), "core:test".into()],
            combination: Rev::new("c13"),
            submissions: vec![13],
            timed_out,
            logs: Vec::new(),
        };
        assert_eq!(
            notice(13, "main", &failed(false)),
            "[sluice] #13 failed on main: cli:test, core:test. sluice wait 13 shows why."
        );
        assert_eq!(
            notice(13, "main", &failed(true)),
            "[sluice] #13 failed on main: timed out. sluice wait 13 shows why."
        );
        let from = Submitter {
            agent: "a".into(),
            worktree: "unit/b".into(),
            path: None,
            head: None,
            host: None,
            session: None,
        };
        let conflict = Decided::Conflict {
            paths: vec!["src/lib.rs".into()],
            with: vec![Contested { id: 11, from }],
        };
        assert_eq!(
            notice(14, "main", &conflict),
            "[sluice] #14 conflicts on main with #11 in src/lib.rs. sluice wait 14 shows it."
        );
        let with_the_branch = Decided::Conflict {
            paths: vec!["src/lib.rs".into()],
            with: Vec::new(),
        };
        assert_eq!(
            notice(14, "main", &with_the_branch),
            "[sluice] #14 conflicts on main in src/lib.rs. sluice wait 14 shows it."
        );
    }
}
