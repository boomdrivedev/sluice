//! A [`Forge`] that keeps one repository's pull requests in a JSON file,
//! for tests: what GitHub would hold, and what a test does to it as a
//! person would, opening, reviewing, checking, drafting and closing.
//! Every call reads the file and every change writes it, so a fake API
//! answering from it and the test that changes it see one GitHub. A change
//! GitHub records as one of the pull request's, a push, a review, a
//! comment, an edit, a draft or ready mark, a close, moves its
//! `updated_at`, as GitHub's does; another system's check moves nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::app::Repo;
use super::forge::{
    latest_opinionated, Check, Comment, Forge, Posted, Posting, Pull, Request, Review, Standing,
    Status,
};

/// The author this fake gives every pull request sluice opens.
pub const LOGIN: &str = "sluice";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Held {
    pub next: u64,
    pub pulls: Vec<Pull>,
    pub reviews: BTreeMap<u64, Vec<Review>>,
    /// Other systems' checks, by revision.
    pub checks: BTreeMap<String, Vec<Check>>,
    /// sluice's check runs, by revision.
    pub posted: BTreeMap<String, Vec<Posted>>,
    /// Every post of sluice's check runs, in order: the revision, the
    /// check's name and what.
    pub posts: Vec<(String, String, Posting)>,
    pub readied: BTreeMap<u64, Vec<DateTime<Utc>>>,
    pub comments: BTreeMap<u64, Vec<Comment>>,
    /// Every edit of a comment, in order: its id and new body.
    #[serde(default)]
    pub edits: Vec<(u64, String)>,
}

pub struct FileForge {
    path: PathBuf,
    repo: String,
    lock: Mutex<()>,
    /// The time a change is stamped with, where a test's clock sets it;
    /// the system's time otherwise.
    at: Mutex<Option<DateTime<Utc>>>,
    /// What it answers an error for, as GitHub may.
    failing: Mutex<Failing>,
}

/// What a [`FileForge`] answers an error for.
#[derive(Default)]
struct Failing {
    /// The pull requests whose reviews it cannot read; the query of every
    /// pull request whole fails while one of them does.
    reviews: std::collections::BTreeSet<u64>,
    /// The list of open pull requests.
    pulls: bool,
    /// Each mark of a pull request as a draft or ready for review.
    drafts: bool,
    /// The query of a branch's pull requests whole.
    query: bool,
}

/// GitHub's words for a request its app may not make.
pub const NOT_ACCESSIBLE: &str = "Resource not accessible by integration";

impl FileForge {
    /// The fake GitHub of `repo` kept in `path`, empty when it does not
    /// exist yet.
    pub fn new(path: &Path, repo: &Repo) -> FileForge {
        FileForge {
            path: path.to_path_buf(),
            repo: repo.to_string(),
            lock: Mutex::new(()),
            at: Mutex::new(None),
            failing: Mutex::new(Failing::default()),
        }
    }

    /// Answer an error for pull request `n`'s reviews, or give them again:
    /// the query of every pull request whole fails while one fails.
    pub fn fail_reviews(&self, n: u64, fail: bool) {
        let mut f = self.failing.lock().unwrap();
        match fail {
            true => f.reviews.insert(n),
            false => f.reviews.remove(&n),
        };
    }

    /// Answer an error for the list of open pull requests, or give it
    /// again.
    pub fn fail_pulls(&self, fail: bool) {
        self.failing.lock().unwrap().pulls = fail;
    }

    /// Refuse, or take again, a mark of a pull request as a draft or ready
    /// for review, in GitHub's words for what its app may not do.
    pub fn refuse_drafts(&self, refuse: bool) {
        self.failing.lock().unwrap().drafts = refuse;
    }

    /// Refuse, or answer again, the query of a branch's pull requests
    /// whole, as GitHub refuses one it cannot answer.
    pub fn refuse_query(&self, refuse: bool) {
        self.failing.lock().unwrap().query = refuse;
    }

    /// From now, each change is stamped `at`, as GitHub would stamp it at
    /// that time by the test's clock.
    pub fn stamp_at(&self, at: DateTime<Utc>) {
        *self.at.lock().unwrap() = Some(at);
    }

    /// The time a change is stamped with now.
    fn now(&self) -> DateTime<Utc> {
        self.at.lock().unwrap().unwrap_or_else(Utc::now)
    }

    pub fn held(&self) -> Result<Held> {
        let _g = self.lock.lock().unwrap();
        self.read()
    }

    fn read(&self) -> Result<Held> {
        match std::fs::read(&self.path) {
            Ok(b) => serde_json::from_slice(&b).with_context(|| self.path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Held {
                next: 1,
                ..Held::default()
            }),
            Err(e) => Err(e).with_context(|| self.path.display().to_string()),
        }
    }

    /// Read, change and write the file.
    pub fn change<T>(&self, f: impl FnOnce(&mut Held) -> Result<T>) -> Result<T> {
        let _g = self.lock.lock().unwrap();
        let mut held = self.read()?;
        let out = f(&mut held)?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&held)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(out)
    }

    /// Change pull request `number`, which moves its `updated_at`.
    fn with_pull<T>(&self, number: u64, f: impl FnOnce(&mut Pull) -> T) -> Result<T> {
        self.change(|h| {
            let p = h
                .pulls
                .iter_mut()
                .find(|p| p.number == number)
                .ok_or_else(|| anyhow!("no pull request {number}"))?;
            p.updated_at = Some(self.now());
            Ok(f(p))
        })
    }

    /// A person opens a pull request into `base` from `head_ref` at
    /// `head`, in `from` (`None` for this repository itself); returns its
    /// number.
    pub fn open_by(
        &self,
        author: &str,
        base: &str,
        head_ref: &str,
        head: &str,
        draft: bool,
        from: Option<&str>,
    ) -> Result<u64> {
        let repo = self.repo.clone();
        self.change(|h| {
            let number = h.next;
            h.next += 1;
            h.pulls.push(Pull {
                number,
                node_id: format!("PR_{number}"),
                title: head_ref.into(),
                body: String::new(),
                author: author.into(),
                draft,
                open: true,
                head: head.into(),
                head_ref: head_ref.into(),
                head_repo: from.map_or(repo, str::to_string),
                base: base.into(),
                updated_at: Some(self.now()),
                merged: false,
            });
            Ok(number)
        })
    }

    /// A commit pushed to pull request `number`'s branch.
    pub fn push(&self, number: u64, head: &str) -> Result<()> {
        self.with_pull(number, |p| p.head = head.into())
    }

    /// Mark it a draft, or ready for review, which GitHub's timeline
    /// records.
    pub fn set_draft(&self, number: u64, draft: bool) -> Result<()> {
        self.change(|h| {
            let p = h
                .pulls
                .iter_mut()
                .find(|p| p.number == number)
                .ok_or_else(|| anyhow!("no pull request {number}"))?;
            if p.draft && !draft {
                h.readied.entry(number).or_default().push(self.now());
            }
            p.draft = draft;
            p.updated_at = Some(self.now());
            Ok(())
        })
    }

    pub fn close(&self, number: u64) -> Result<()> {
        self.with_pull(number, |p| p.open = false)
    }

    /// Closed as merged: its head is in the branch it is into.
    pub fn merge(&self, number: u64) -> Result<()> {
        self.with_pull(number, |p| {
            p.open = false;
            p.merged = true;
        })
    }

    /// Pull request `number` changed on GitHub in a way sluice reads.
    fn touch(&self, h: &mut Held, number: u64) {
        if let Some(p) = h.pulls.iter_mut().find(|p| p.number == number) {
            p.updated_at = Some(self.now());
        }
    }

    /// `user` reviews pull request `number` on `commit`.
    pub fn review(&self, number: u64, user: &str, state: &str, commit: &str) -> Result<()> {
        self.change(|h| {
            self.touch(h, number);
            h.reviews.entry(number).or_default().push(Review {
                user: user.into(),
                state: state.into(),
                commit: commit.into(),
            });
            Ok(())
        })
    }

    /// Another system posts the check `name` on `rev`.
    pub fn check(&self, rev: &str, name: &str, standing: Standing) -> Result<()> {
        self.change(|h| {
            let cs = h.checks.entry(rev.into()).or_default();
            cs.retain(|c| c.name != name);
            cs.push(Check {
                name: name.into(),
                standing,
            });
            Ok(())
        })
    }

    /// The pull request from `head_ref`, open or closed, the latest.
    pub fn by_ref(&self, head_ref: &str) -> Result<Option<Pull>> {
        Ok(self
            .held()?
            .pulls
            .into_iter()
            .rev()
            .find(|p| p.head_ref == head_ref))
    }
}

impl Forge for FileForge {
    fn login(&self, _: &Repo) -> Result<String> {
        Ok(LOGIN.into())
    }

    fn open_pulls(&self, _: &Repo) -> Result<Vec<Pull>> {
        if self.failing.lock().unwrap().pulls {
            bail!("GitHub answered 502 Bad Gateway for the list of pull requests");
        }
        Ok(self.held()?.pulls.into_iter().filter(|p| p.open).collect())
    }

    /// Every open pull request into `base` whole, as GitHub's one query
    /// reads them: each reviewer's latest review that decides anything,
    /// and the last ten ready marks.
    fn requests(&self, _: &Repo, base: &str) -> Result<Vec<Request>> {
        {
            let f = self.failing.lock().unwrap();
            if f.query || !f.reviews.is_empty() {
                bail!("GitHub could not answer the query of {base}'s pull requests");
            }
        }
        let mut held = self.held()?;
        let pulls: Vec<Pull> = held
            .pulls
            .iter()
            .filter(|p| p.open && p.base == base)
            .cloned()
            .collect();
        Ok(pulls
            .into_iter()
            .map(|p| {
                let mut readied = held.readied.remove(&p.number).unwrap_or_default();
                readied.drain(..readied.len().saturating_sub(10));
                Request {
                    reviews: latest_opinionated(
                        &held.reviews.remove(&p.number).unwrap_or_default(),
                    ),
                    checks: held.checks.get(&p.head).cloned().unwrap_or_default(),
                    posted: held.posted.get(&p.head).cloned().unwrap_or_default(),
                    readied,
                    pull: p,
                }
            })
            .collect())
    }

    fn pull(&self, _: &Repo, number: u64) -> Result<Option<Pull>> {
        Ok(self.held()?.pulls.into_iter().find(|p| p.number == number))
    }

    fn reviews(&self, _: &Repo, number: u64) -> Result<Vec<Review>> {
        if self.failing.lock().unwrap().reviews.contains(&number) {
            bail!("GitHub answered 502 Bad Gateway for pull request {number}'s reviews");
        }
        Ok(self.held()?.reviews.remove(&number).unwrap_or_default())
    }

    fn checks(&self, _: &Repo, rev: &str) -> Result<Vec<Check>> {
        Ok(self.held()?.checks.remove(rev).unwrap_or_default())
    }

    fn readied(&self, _: &Repo, number: u64) -> Result<Vec<DateTime<Utc>>> {
        Ok(self.held()?.readied.remove(&number).unwrap_or_default())
    }

    fn posted_all(&self, _: &Repo, rev: &str) -> Result<Vec<Posted>> {
        Ok(self.held()?.posted.remove(rev).unwrap_or_default())
    }

    /// Post the check run `name` on `rev`, or patch the run `existing`. A
    /// completed run, `Failure` or `Success`, patched `Queued` or
    /// `InProgress` stays completed and takes only the posting's title,
    /// summary and text, as GitHub kept WilldanGroup/switchboard-kit #268's
    /// check on `977138a9` failed under `waiting: the next submit` (record
    /// 0096). Any other posting replaces the run whole. `posts` keeps what
    /// sluice sent.
    fn post_named(
        &self,
        _: &Repo,
        rev: &str,
        name: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64> {
        self.change(|h| {
            let id = match existing {
                Some(id) => id,
                None => {
                    let id = h.next;
                    h.next += 1;
                    id
                }
            };
            let on = h.posted.entry(rev.into()).or_default();
            let mut kept = posting.clone();
            if let Some(was) = on.iter().find(|p| p.id == id) {
                let completed = matches!(was.posting.status, Status::Failure | Status::Success);
                let undecided = matches!(posting.status, Status::Queued | Status::InProgress);
                if completed && undecided {
                    kept.status = was.posting.status;
                }
            }
            on.retain(|p| p.id != id);
            on.push(Posted {
                id,
                name: name.into(),
                posting: kept,
            });
            h.posts.push((rev.into(), name.into(), posting.clone()));
            Ok(id)
        })
    }

    fn comment_list(&self, _: &Repo, number: u64) -> Result<Vec<Comment>> {
        Ok(self.held()?.comments.remove(&number).unwrap_or_default())
    }

    fn comment(&self, repo: &Repo, number: u64, body: &str) -> Result<u64> {
        self.change(|h| {
            self.touch(h, number);
            let id = h.next;
            h.next += 1;
            h.comments.entry(number).or_default().push(Comment {
                id,
                body: body.into(),
                url: format!("https://github.com/{repo}/pull/{number}#issuecomment-{id}"),
            });
            Ok(id)
        })
    }

    fn edit_comment(&self, _: &Repo, id: u64, body: &str) -> Result<()> {
        self.change(|h| {
            let c = h
                .comments
                .values_mut()
                .flatten()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow!("no comment {id}"))?;
            c.body = body.into();
            h.edits.push((id, body.into()));
            Ok(())
        })
    }

    fn open(
        &self,
        repo: &Repo,
        base: &str,
        head_ref: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<Pull> {
        // GitHub reads the head from the branch as pushed; the test's push
        // is to a bare remote this fake cannot see, so it is named after.
        let number = self.open_by(LOGIN, base, head_ref, "", draft, None)?;
        self.with_pull(number, |p| {
            p.title = title.into();
            p.body = body.into();
        })?;
        self.pull(repo, number)?
            .ok_or_else(|| anyhow!("no pull request {number}"))
    }

    fn edit(&self, _: &Repo, number: u64, title: Option<&str>, body: &str) -> Result<()> {
        self.with_pull(number, |p| {
            if let Some(title) = title {
                p.title = title.into();
            }
            p.body = body.into();
        })
    }

    fn set_draft(&self, _: &Repo, number: u64, draft: bool) -> Result<()> {
        if self.failing.lock().unwrap().drafts {
            bail!("{NOT_ACCESSIBLE}");
        }
        FileForge::set_draft(self, number, draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::github::forge::{approvals, Status};

    fn forge(name: &str) -> (FileForge, Repo, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("sluice-fake-forge-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        (FileForge::new(&dir.join("github.json"), &repo), repo, dir)
    }

    /// Open pull requests into a branch are listed with what intake reads;
    /// closed ones and those into other branches are not, and a fork's is
    /// known as one.
    #[test]
    fn pull_requests_are_listed() {
        let (f, repo, dir) = forge("list");
        let a = f
            .open_by("ann", "main", "unit/a", "aaa", false, None)
            .unwrap();
        let b = f
            .open_by("bob", "main", "unit/b", "bbb", true, None)
            .unwrap();
        f.open_by("cat", "bolt/x", "unit/c", "ccc", false, None)
            .unwrap();
        let d = f
            .open_by("dan", "main", "unit/d", "ddd", false, Some("dan/r"))
            .unwrap();
        f.close(b).unwrap();
        let open = f.pulls(&repo, "main").unwrap();
        assert_eq!(open.iter().map(|p| p.number).collect::<Vec<_>>(), [a, d]);
        assert!(open[0].from_repo(&repo) && !open[1].from_repo(&repo));
        assert!(!f.pull(&repo, b).unwrap().unwrap().open);
        f.push(a, "aa2").unwrap();
        assert_eq!(f.pull(&repo, a).unwrap().unwrap().head, "aa2");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 136: the one query's answer holds what the reads per request do:
    /// each request's reviews as `approvals` counts them, the checks and
    /// sluice's check runs on its head, and its ready marks.
    #[test]
    fn requests_read_whole() {
        let (f, repo, dir) = forge("requests");
        let a = f
            .open_by("ann", "main", "unit/a", "aaa", true, None)
            .unwrap();
        f.open_by("cat", "bolt/x", "unit/c", "ccc", false, None)
            .unwrap();
        f.set_draft(a, false).unwrap();
        f.review(a, "bob", "APPROVED", "aaa").unwrap();
        f.review(a, "cat", "APPROVED", "old").unwrap();
        f.review(a, "cat", "COMMENTED", "aaa").unwrap();
        f.check("aaa", "qa/approved", Standing::Passed).unwrap();
        let waiting = Posting {
            status: Status::Queued,
            title: "waiting".into(),
            summary: String::new(),
            text: None,
        };
        f.post(&repo, "aaa", None, &waiting).unwrap();
        let rs = f.requests(&repo, "main").unwrap();
        assert_eq!(rs.len(), 1);
        let r = &rs[0];
        assert_eq!(r.pull, f.pull(&repo, a).unwrap().unwrap());
        assert_eq!(
            approvals(&r.reviews, "aaa"),
            approvals(&f.reviews(&repo, a).unwrap(), "aaa")
        );
        assert_eq!(r.checks, f.checks(&repo, "aaa").unwrap());
        assert_eq!(r.posted, f.posted_all(&repo, "aaa").unwrap());
        assert_eq!(r.readied, f.readied(&repo, a).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reviews_on_a_head() {
        let (f, repo, dir) = forge("reviews");
        let n = f
            .open_by("ann", "main", "unit/a", "old", false, None)
            .unwrap();
        f.review(n, "bob", "APPROVED", "old").unwrap();
        f.push(n, "new").unwrap();
        let rs = f.reviews(&repo, n).unwrap();
        assert_eq!(approvals(&rs, "new"), 0);
        f.review(n, "bob", "APPROVED", "new").unwrap();
        assert_eq!(approvals(&f.reviews(&repo, n).unwrap(), "new"), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn checks_by_name() {
        let (f, repo, dir) = forge("checks");
        f.check("aaa", "qa/approved", Standing::Pending).unwrap();
        f.check("aaa", "ci", Standing::Failed).unwrap();
        f.check("aaa", "qa/approved", Standing::Passed).unwrap();
        let cs = f.checks(&repo, "aaa").unwrap();
        assert_eq!(cs.len(), 2);
        assert!(cs
            .iter()
            .any(|c| c.name == "qa/approved" && c.standing == Standing::Passed));
        assert!(f.checks(&repo, "bbb").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ready_for_review_events() {
        let (f, repo, dir) = forge("ready");
        let n = f
            .open_by("ann", "main", "unit/a", "aaa", true, None)
            .unwrap();
        assert!(f.readied(&repo, n).unwrap().is_empty());
        f.set_draft(n, false).unwrap();
        f.set_draft(n, false).unwrap();
        f.set_draft(n, true).unwrap();
        f.set_draft(n, false).unwrap();
        assert_eq!(f.readied(&repo, n).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Through the forge interface, as sluice marks it: a draft and then
    /// ready again is one new ready time on the timeline.
    #[test]
    fn a_draft_then_ready_is_one_ready_time() {
        let (f, repo, dir) = forge("draft-ready");
        let n = f
            .open_by("ann", "main", "unit/a", "aaa", false, None)
            .unwrap();
        let forge: &dyn Forge = &f;
        forge.set_draft(&repo, n, true).unwrap();
        assert!(forge.pull(&repo, n).unwrap().unwrap().draft);
        assert!(forge.readied(&repo, n).unwrap().is_empty());
        forge.set_draft(&repo, n, false).unwrap();
        let p = forge.pull(&repo, n).unwrap().unwrap();
        assert!(!p.draft);
        assert_eq!(p.node_id, format!("PR_{n}"));
        assert_eq!(forge.readied(&repo, n).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// sluice's check run is found by its head and updated in place; every
    /// post is kept, for a test to count.
    #[test]
    fn posting_a_status_and_a_comment() {
        let (f, repo, dir) = forge("post");
        let n = f
            .open_by("ann", "main", "unit/a", "aaa", false, None)
            .unwrap();
        assert_eq!(f.posted(&repo, "aaa").unwrap(), None);
        let waiting = Posting {
            status: Status::Queued,
            title: "waiting".into(),
            summary: "needs 1 review".into(),
            text: None,
        };
        f.post(&repo, "aaa", None, &waiting).unwrap();
        let first = f.posted(&repo, "aaa").unwrap().unwrap();
        assert_eq!(first.posting, waiting);
        let landed = Posting {
            status: Status::Success,
            title: "landed".into(),
            summary: String::new(),
            text: Some("the report".into()),
        };
        f.post(&repo, "aaa", Some(first.id), &landed).unwrap();
        let second = f.posted(&repo, "aaa").unwrap().unwrap();
        assert_eq!((second.id, second.posting), (first.id, landed));
        assert_eq!(f.held().unwrap().posts.len(), 2);
        f.comment(&repo, n, "blamed").unwrap();
        assert_eq!(f.comments(&repo, n).unwrap(), ["blamed"]);
        // ADR 0044: a check per check, beside `sluice`, and a comment
        // edited in place.
        f.post_named(&repo, "aaa", "sluice / a:test", None, &waiting)
            .unwrap();
        let all = f.posted_all(&repo, "aaa").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(f.posted(&repo, "aaa").unwrap().unwrap().id, first.id);
        let c = f.comment_list(&repo, n).unwrap().remove(0);
        f.edit_comment(&repo, c.id, "edited").unwrap();
        assert_eq!(f.comments(&repo, n).unwrap(), ["edited"]);
        let opened = f.open(&repo, "main", "unit/b", "B", "body", false).unwrap();
        assert!(!opened.draft);
        assert_eq!(opened.author, f.login(&repo).unwrap());
        f.edit(&repo, opened.number, None, "new body").unwrap();
        let edited = f.by_ref("unit/b").unwrap().unwrap();
        assert_eq!(
            (edited.title.as_str(), edited.body.as_str()),
            ("B", "new body")
        );
        f.edit(&repo, opened.number, Some("B2"), "new body")
            .unwrap();
        assert_eq!(f.by_ref("unit/b").unwrap().unwrap().title, "B2");
        let drafted = f.open(&repo, "main", "unit/c", "C", "", true).unwrap();
        assert!(drafted.draft && f.by_ref("unit/c").unwrap().unwrap().draft);
        assert!(f.readied(&repo, drafted.number).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Record 0096: a run posted `Failure` and patched `Queued` stays a
    /// failure under the queued posting's words, as GitHub kept
    /// switchboard-kit #268's; a decided posting replaces it whole.
    #[test]
    fn a_completed_check_run_stays_completed() {
        let (f, repo, dir) = forge("completed");
        let failed = Posting {
            status: Status::Failure,
            title: "failed: a:test".into(),
            summary: "a:test failed".into(),
            text: Some("the report".into()),
        };
        let id = f.post(&repo, "aaa", None, &failed).unwrap();
        let held = Posting {
            status: Status::Queued,
            title: "waiting: the next submit".into(),
            summary: "Its submission left the queue".into(),
            text: None,
        };
        f.post(&repo, "aaa", Some(id), &held).unwrap();
        let now = f.posted(&repo, "aaa").unwrap().unwrap();
        assert_eq!(now.id, id);
        assert_eq!(
            now.posting,
            Posting {
                status: Status::Failure,
                ..held.clone()
            }
        );
        let testing = Posting {
            status: Status::InProgress,
            title: "testing".into(),
            summary: String::new(),
            text: None,
        };
        f.post(&repo, "aaa", Some(id), &testing).unwrap();
        assert_eq!(
            f.posted(&repo, "aaa").unwrap().unwrap().posting.status,
            Status::Failure
        );
        let landed = Posting {
            status: Status::Success,
            title: "landed".into(),
            summary: String::new(),
            text: None,
        };
        f.post(&repo, "aaa", Some(id), &landed).unwrap();
        assert_eq!(f.posted(&repo, "aaa").unwrap().unwrap().posting, landed);
        f.post(&repo, "aaa", Some(id), &held).unwrap();
        assert_eq!(
            f.posted(&repo, "aaa").unwrap().unwrap().posting.status,
            Status::Success
        );
        assert_eq!(f.held().unwrap().posts[1].2, held);
        let _ = std::fs::remove_dir_all(dir);
    }
}
