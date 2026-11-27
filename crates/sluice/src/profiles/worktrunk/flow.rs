//! What `wt submit` does once its command line is read ([`Submit`]): it
//! reads what stands of the worktree's last submission, submits or
//! follows, and removes the worktree once sluice records it landed (C.4,
//! C.5 worktrunk; record 0065). It reaches sluice, worktrunk and the
//! terminal through [`Tools`] alone, which the `wt-submit` binary gives as
//! processes.

use std::ffi::OsString;
use std::path::Path;

use anyhow::Result;

use super::{Single, Standing, Submit};
use crate::{INTERNAL, LANDED, USAGE};

/// The exit of a run whose `sluice` could not be started.
pub const NO_SLUICE: i32 = 127;

/// What `wt submit` runs and where it writes.
pub trait Tools {
    /// `sluice` with `args`, its output the run's own: its exit.
    fn sluice(&self, args: &[OsString]) -> Result<i32>;

    /// `sluice` with `args`, its standard error the run's own: its exit
    /// and its standard output.
    fn sluice_output(&self, args: &[OsString]) -> Result<(i32, Vec<u8>)>;

    /// What stands of `worktree`'s last submission to `branch`, or why it
    /// could not be read.
    fn standing(&self, branch: &str, worktree: &Path) -> std::result::Result<Standing, String>;

    /// Whether worktrunk removes `worktree` once it lands.
    fn removes_on_landing(&self, worktree: &Path) -> bool;

    /// `sluice release` of `worktree`.
    fn release(&self, worktree: &Path) -> Result<()>;

    /// `wt remove` of `worktree`.
    fn remove(&self, worktree: &Path) -> Result<()>;

    /// A line on the run's standard error.
    fn say(&self, line: &str);

    /// Bytes on the run's standard output.
    fn print(&self, bytes: &[u8]);

    /// Bytes on the run's standard error.
    fn print_err(&self, bytes: &[u8]);
}

/// `wt submit` as `submit` reads its command line: its exit.
pub fn run(submit: &Submit, tools: &dyn Tools) -> i32 {
    if let Some(why) = &submit.refused {
        tools.say(&format!("wt submit: {why}"));
        return USAGE;
    }
    let Some(one) = &submit.single else {
        return sluice(tools, &submit.sluice);
    };
    // 51, D5: a landing whose worktree goes is confirmed on receipt.
    let confirms = one.waits && one.remove && tools.removes_on_landing(one.worktree.as_path());
    let mut sluice_args = submit.sluice.clone();
    if confirms {
        sluice_args.push("--confirm-landing".into());
    }
    // Never a guess: a submit at the revision standing withdraws it (67).
    let standing = match tools.standing(&one.branch, &one.worktree) {
        Ok(standing) => standing,
        Err(e) => {
            tools.say(&format!(
                "wt submit: could not read whether a submission stands, so nothing was \
                 submitted: {e}"
            ));
            return INTERNAL;
        }
    };
    let code = match standing {
        Standing::Nothing if one.waits && one.timeout.is_some() => {
            // A first submit under a time limit: submitted without waiting,
            // what that says on stderr, so stdout is the wait's one
            // document; then followed.
            let mut args = submit.sluice.clone();
            args.push("--no-wait".into());
            let (code, stdout) = match tools.sluice_output(&args) {
                Ok(out) => out,
                Err(err) => {
                    tools.say(&format!("wt submit: running sluice: {err}"));
                    return NO_SLUICE;
                }
            };
            if code != 0 {
                tools.print(&stdout);
                return code;
            }
            tools.print_err(&stdout);
            follow(tools, one, confirms)
        }
        Standing::Nothing => sluice(tools, &sluice_args),
        standing if one.orders => {
            let why = match standing {
                Standing::Submission { id, .. } => format!(
                    "submission {id} stands at this revision; change its place with \
                     sluice amend {id}"
                ),
                _ => format!(
                    "{} stands at this revision, and only a new head submits again",
                    named(&standing)
                ),
            };
            tools.say(&format!("wt submit: {why}"));
            return USAGE;
        }
        _ if !one.waits => {
            // Answered with what stands, and nothing removed: only a run
            // that waits removes.
            let mut args: Vec<OsString> = vec![
                "submit".into(),
                one.branch.clone().into(),
                "--status".into(),
                "--worktree".into(),
                one.worktree.clone().into(),
            ];
            if one.json {
                args.push("--json".into());
            }
            return sluice(tools, &args);
        }
        standing => {
            let what = match standing {
                Standing::PullRequest { landed: true, .. } => "landed",
                Standing::PullRequest { .. } => "already open",
                _ => "already submitted",
            };
            tools.say(&format!(
                "wt submit: following {}, {what} at this revision",
                named(&standing)
            ));
            follow(tools, one, confirms)
        }
    };
    if code == LANDED && one.waits && one.remove {
        let w = one.worktree.as_path();
        if !landed(tools, &one.branch, w) {
            tools.say(
                "wt submit: sluice records no landing of this worktree's head, so it was kept",
            );
        } else if tools.removes_on_landing(w) {
            // A driver that removes a session's place releases it first.
            if let Err(e) = tools.release(w) {
                tools.say(&format!(
                    "wt submit: landed, but the worktree was not released, so it was kept: {e}"
                ));
            } else if let Err(e) = tools.remove(w) {
                tools.say(&format!(
                    "wt submit: landed, but the worktree was not removed: {e}"
                ));
            }
        }
    }
    code
}

/// `sluice` with `args`: its exit, or [`NO_SLUICE`] where it could not
/// start.
fn sluice(tools: &dyn Tools, args: &[OsString]) -> i32 {
    match tools.sluice(args) {
        Ok(code) => code,
        Err(err) => {
            tools.say(&format!("wt submit: running sluice: {err}"));
            NO_SLUICE
        }
    }
}

/// Whether sluice records `worktree`'s submission to `branch` as landed:
/// its last submission from the worktree, or its pull request another
/// host took in (137; record 0096), has landed, and is of the revision a
/// submit from the worktree would send now. A worktree is removed on
/// this, never on an exit alone.
fn landed(tools: &dyn Tools, branch: &str, worktree: &Path) -> bool {
    matches!(
        tools.standing(branch, worktree),
        Ok(Standing::Submission { landed: true, .. } | Standing::PullRequest { landed: true, .. })
    )
}

/// `sluice wait <branch>` from the worktree, under `--timeout` and `--json`
/// when given: it follows the worktree's last submission, a pull request
/// not yet taken in among them, as no `sluice wait <id>` can; confirming on
/// receipt a landing whose worktree goes, where `confirms`.
fn follow(tools: &dyn Tools, one: &Single, confirms: bool) -> i32 {
    let mut args: Vec<OsString> = vec![
        "wait".into(),
        one.branch.clone().into(),
        "--worktree".into(),
        one.worktree.clone().into(),
    ];
    if let Some(t) = &one.timeout {
        args.extend(["--timeout".into(), t.clone()]);
    }
    if one.json {
        args.push("--json".into());
    }
    if confirms {
        args.push("--confirm-landing".into());
    }
    sluice(tools, &args)
}

/// What stands, as the run names it.
fn named(standing: &Standing) -> String {
    match standing {
        Standing::Submission { id, .. } => format!("submission {id}"),
        Standing::PullRequest { number, .. } => format!("pull request #{number}"),
        Standing::Nothing => "nothing".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::{FAILED, WAITING};

    /// [`Tools`] that answer as a test sets and keep what was asked.
    #[derive(Default)]
    struct Fake {
        /// What each read of what stands answers, in order; the last
        /// answers every read after it.
        standing: RefCell<VecDeque<std::result::Result<Standing, String>>>,
        /// The exit of `sluice submit` and of `sluice wait`.
        submit: i32,
        wait: i32,
        /// What `sluice submit --no-wait` prints on its standard output.
        submitted: Vec<u8>,
        keeps: bool,
        release_fails: bool,
        /// Each call, in order, as one line.
        calls: RefCell<Vec<String>>,
        said: RefCell<Vec<String>>,
        out: RefCell<Vec<u8>>,
        err: RefCell<Vec<u8>>,
    }

    impl Fake {
        fn standing(answers: Vec<std::result::Result<Standing, String>>) -> Fake {
            Fake {
                standing: RefCell::new(answers.into()),
                ..Fake::default()
            }
        }

        fn call(&self, line: String) {
            self.calls.borrow_mut().push(line);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn said(&self) -> Vec<String> {
            self.said.borrow().clone()
        }
    }

    fn line(args: &[OsString]) -> String {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    impl Tools for Fake {
        fn sluice(&self, args: &[OsString]) -> Result<i32> {
            let l = line(args);
            self.call(format!("sluice {l}"));
            Ok(match args[0].to_str() {
                Some("wait") => self.wait,
                _ if l.contains("--status") => 0,
                _ => self.submit,
            })
        }

        fn sluice_output(&self, args: &[OsString]) -> Result<(i32, Vec<u8>)> {
            self.call(format!("sluice {} (output)", line(args)));
            Ok((self.submit, self.submitted.clone()))
        }

        fn standing(&self, _: &str, _: &Path) -> std::result::Result<Standing, String> {
            self.call("standing".into());
            let mut q = self.standing.borrow_mut();
            match q.len() {
                0 => Ok(Standing::Nothing),
                1 => q[0].clone(),
                _ => q.pop_front().unwrap(),
            }
        }

        fn removes_on_landing(&self, _: &Path) -> bool {
            !self.keeps
        }

        fn release(&self, w: &Path) -> Result<()> {
            self.call(format!("release {}", w.display()));
            if self.release_fails {
                anyhow::bail!("sluice release exited 3");
            }
            Ok(())
        }

        fn remove(&self, w: &Path) -> Result<()> {
            self.call(format!("remove {}", w.display()));
            Ok(())
        }

        fn say(&self, line: &str) {
            self.said.borrow_mut().push(line.to_string());
        }

        fn print(&self, bytes: &[u8]) {
            self.out.borrow_mut().extend_from_slice(bytes);
        }

        fn print_err(&self, bytes: &[u8]) {
            self.err.borrow_mut().extend_from_slice(bytes);
        }
    }

    fn submit(args: &[&str]) -> Submit {
        Submit::parse(
            ["bolt/x", "--worktree", "/w/a"]
                .iter()
                .chain(args)
                .map(OsString::from),
        )
    }

    fn landed(id: u64) -> std::result::Result<Standing, String> {
        Ok(Standing::Submission { id, landed: true })
    }

    fn queued(id: u64) -> std::result::Result<Standing, String> {
        Ok(Standing::Submission { id, landed: false })
    }

    /// 1, 47, C.4, clause 95, 51 (record 0102, D5): a waiting submit where
    /// nothing stands submits, confirming a landing whose worktree goes on
    /// receipt; once sluice records the landing it releases the worktree,
    /// then removes it (`wt_submit_lands_or_fails_by_its_exit`,
    /// `wt_submit_releases_before_it_removes`,
    /// `wt_submit_confirms_a_landing_it_removes_the_worktree_of`).
    #[test]
    fn a_landing_is_released_then_removed() {
        let f = Fake {
            submit: LANDED,
            ..Fake::standing(vec![Ok(Standing::Nothing), landed(1)])
        };
        assert_eq!(run(&submit(&[]), &f), LANDED);
        assert_eq!(
            f.calls(),
            [
                "standing",
                "sluice submit bolt/x --worktree /w/a --confirm-landing",
                "standing",
                "release /w/a",
                "remove /w/a",
            ]
        );
    }

    /// C.4: under `--no-remove`, or where worktrunk's `[merge] remove` is
    /// false, a landing removes nothing, releases nothing, and confirms
    /// nothing on receipt (`run_again_once_landed_keeps_under_no_remove`).
    #[test]
    fn a_kept_worktree_is_neither_released_nor_removed() {
        for (args, keeps) in [(&["--no-remove"][..], false), (&[][..], true)] {
            let f = Fake {
                submit: LANDED,
                keeps,
                ..Fake::standing(vec![Ok(Standing::Nothing), landed(1)])
            };
            assert_eq!(run(&submit(args), &f), LANDED);
            let calls = f.calls();
            assert!(
                calls
                    .iter()
                    .all(|c| !c.starts_with("release") && !c.starts_with("remove")),
                "{args:?}: {calls:?}"
            );
            assert!(
                !calls.iter().any(|c| c.contains("--confirm-landing")),
                "{calls:?}"
            );
        }
    }

    /// 47, S3: a blame exits as sluice's submit does, and keeps the
    /// worktree (`wt_submit_lands_or_fails_by_its_exit`).
    #[test]
    fn a_blame_keeps_the_worktree() {
        let f = Fake {
            submit: FAILED,
            ..Fake::default()
        };
        assert_eq!(run(&submit(&[]), &f), FAILED);
        assert_eq!(f.calls().len(), 2, "{:?}", f.calls());
    }

    /// Record 0065: run again where a submission stands, it submits
    /// nothing, follows that submission with `sluice wait <branch>` from
    /// the worktree, says so, and removes the worktree once it lands
    /// (`run_again_follows_its_submission_and_removes_on_landing`,
    /// `run_again_once_landed_removes_at_once`).
    #[test]
    fn a_run_again_follows_what_stands() {
        for stands in [queued(4), landed(4)] {
            let f = Fake {
                wait: LANDED,
                ..Fake::standing(vec![stands, landed(4)])
            };
            assert_eq!(run(&submit(&[]), &f), LANDED);
            assert_eq!(
                f.calls(),
                [
                    "standing",
                    "sluice wait bolt/x --worktree /w/a --confirm-landing",
                    "standing",
                    "release /w/a",
                    "remove /w/a",
                ]
            );
            assert_eq!(
                f.said(),
                ["wt submit: following submission 4, already submitted at this revision"]
            );
        }
    }

    /// A pull request standing is followed, named by its number.
    #[test]
    fn a_pull_request_standing_is_followed() {
        let f = Fake {
            wait: FAILED,
            ..Fake::standing(vec![Ok(Standing::PullRequest {
                number: 7,
                landed: false,
            })])
        };
        assert_eq!(run(&submit(&[]), &f), FAILED);
        assert_eq!(
            f.said(),
            ["wt submit: following pull request #7, already open at this revision"]
        );
    }

    /// Blamed while followed: the run exits as `sluice wait` does, and the
    /// worktree stays (`run_again_blamed_keeps_the_worktree`).
    #[test]
    fn a_followed_blame_keeps_the_worktree() {
        let f = Fake {
            wait: FAILED,
            ..Fake::standing(vec![queued(2)])
        };
        assert_eq!(run(&submit(&["--timeout", "24h"]), &f), FAILED);
        assert_eq!(
            f.calls(),
            [
                "standing",
                "sluice wait bolt/x --worktree /w/a --timeout 24h --confirm-landing",
            ]
        );
    }

    /// A first submit under a time limit submits without waiting, its
    /// answer on stderr, then follows under the limit; out of time, it
    /// exits 5 and keeps the worktree
    /// (`a_first_submit_under_a_time_limit_submits_then_follows`).
    #[test]
    fn a_first_submit_under_a_time_limit_submits_then_follows() {
        let f = Fake {
            submit: 0,
            wait: WAITING,
            submitted: b"submission 1 to bolt/x\n".to_vec(),
            ..Fake::default()
        };
        assert_eq!(run(&submit(&["--timeout", "2s", "--json"]), &f), WAITING);
        assert_eq!(
            f.calls(),
            [
                "standing",
                "sluice submit bolt/x --worktree /w/a --json --no-wait (output)",
                "sluice wait bolt/x --worktree /w/a --timeout 2s --json --confirm-landing",
            ]
        );
        assert_eq!(*f.err.borrow(), b"submission 1 to bolt/x\n");
        assert!(f.out.borrow().is_empty());
    }

    /// A first submit under a time limit that sluice refuses prints its
    /// answer on stdout and exits as it did, following nothing.
    #[test]
    fn a_refused_first_submit_follows_nothing() {
        let f = Fake {
            submit: 2,
            submitted: b"{\"error\":{}}".to_vec(),
            ..Fake::default()
        };
        assert_eq!(run(&submit(&["--timeout", "2s", "--json"]), &f), 2);
        assert_eq!(f.calls().len(), 2, "{:?}", f.calls());
        assert_eq!(*f.out.borrow(), b"{\"error\":{}}");
    }

    /// 67: blamed, the same head submitted again stands nowhere, so a run
    /// again submits it anew (`run_again_after_a_blame_submits_the_same_head`).
    #[test]
    fn nothing_standing_submits_anew() {
        let f = Fake {
            submit: FAILED,
            ..Fake::default()
        };
        assert_eq!(run(&submit(&[]), &f), FAILED);
        assert!(f.said().is_empty(), "{:?}", f.said());
        assert!(f.calls()[1].starts_with("sluice submit"), "{:?}", f.calls());
    }

    /// Record 0065: a `--no-wait` run again answers with what stands, by
    /// `sluice submit --status`, and removes nothing
    /// (`a_no_wait_run_again_answers_with_what_stands`,
    /// `a_no_wait_run_again_once_landed_keeps_the_worktree`).
    #[test]
    fn a_no_wait_run_again_answers_with_what_stands() {
        for stands in [queued(1), landed(1)] {
            let f = Fake::standing(vec![stands]);
            assert_eq!(run(&submit(&["--no-wait", "--json"]), &f), 0);
            assert_eq!(
                f.calls(),
                [
                    "standing",
                    "sluice submit bolt/x --status --worktree /w/a --json",
                ]
            );
        }
    }

    /// Only a new head submits again: `--after` where a submission stands
    /// exits 64 naming `sluice amend <id>`, and runs nothing
    /// (`a_place_in_the_queue_where_one_stands_is_refused`).
    #[test]
    fn a_place_in_the_queue_where_one_stands_is_refused() {
        let f = Fake::standing(vec![queued(1)]);
        assert_eq!(run(&submit(&["--no-wait", "--after", "2"]), &f), USAGE);
        assert_eq!(f.calls(), ["standing"]);
        assert_eq!(
            f.said(),
            [
                "wt submit: submission 1 stands at this revision; change its place with \
              sluice amend 1"
            ]
        );
    }

    /// 67: where what stands cannot be read, nothing is submitted, and the
    /// run exits 3 with sluice's words (`an_unread_status_submits_nothing`).
    #[test]
    fn an_unread_status_submits_nothing() {
        let f = Fake::standing(vec![Err("sluice submit --status exited 3: no".into())]);
        assert_eq!(run(&submit(&["--timeout", "9m"]), &f), INTERNAL);
        assert_eq!(f.calls(), ["standing"]);
        assert_eq!(
            f.said(),
            [
                "wt submit: could not read whether a submission stands, so nothing was \
              submitted: sluice submit --status exited 3: no"
            ]
        );
    }

    /// A worktree whose head sluice records no landing of, a commit made
    /// while it was followed, is kept, and the run says so
    /// (`a_commit_while_followed_keeps_the_worktree`).
    #[test]
    fn a_landing_of_another_head_keeps_the_worktree() {
        let f = Fake {
            wait: LANDED,
            ..Fake::standing(vec![queued(1), Ok(Standing::Nothing)])
        };
        assert_eq!(run(&submit(&[]), &f), LANDED);
        assert!(
            !f.calls().iter().any(|c| c.starts_with("remove")),
            "{:?}",
            f.calls()
        );
        assert_eq!(
            f.said().last().unwrap(),
            "wt submit: sluice records no landing of this worktree's head, so it was kept"
        );
    }

    /// A release that fails keeps the worktree, saying why.
    #[test]
    fn a_failed_release_keeps_the_worktree() {
        let f = Fake {
            submit: LANDED,
            release_fails: true,
            ..Fake::standing(vec![Ok(Standing::Nothing), landed(1)])
        };
        assert_eq!(run(&submit(&[]), &f), LANDED);
        assert!(!f.calls().iter().any(|c| c.starts_with("remove")));
        assert_eq!(
            f.said(),
            [
                "wt submit: landed, but the worktree was not released, so it was kept: \
              sluice release exited 3"
            ]
        );
    }

    /// A command line refused runs nothing and exits 64; help, a version
    /// or arguments sluice refuses go to sluice as they are, read for
    /// nothing (`a_time_limit_without_waiting_is_refused`,
    /// `only_a_landing_removes_the_worktree`).
    #[test]
    fn a_refused_or_unread_command_line_removes_nothing() {
        let f = Fake::default();
        assert_eq!(run(&submit(&["--no-wait", "--timeout", "9m"]), &f), USAGE);
        assert!(f.calls().is_empty());
        assert!(f.said()[0].contains("--timeout is for a run that waits"));
        for args in [&["--help"][..], &["--version"], &["--no-such-flag"]] {
            let f = Fake {
                submit: USAGE,
                ..Fake::default()
            };
            run(&submit(args), &f);
            assert_eq!(f.calls().len(), 1, "{args:?}: {:?}", f.calls());
            assert!(f.calls()[0].starts_with("sluice submit"), "{:?}", f.calls());
        }
    }
}
