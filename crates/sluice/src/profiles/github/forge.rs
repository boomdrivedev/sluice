//! What the pull request driver asks of GitHub (C.5 pull request), behind
//! one interface: [`super::App`] asks GitHub's API as sluice's app, and
//! [`super::fake::FileForge`] answers from a file, for tests.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::app::Repo;

/// The name of the check run sluice posts on a pull request's head.
pub const SLUICE_CHECK: &str = sluice_core::policy::OWN_CHECK;

/// An open or closed pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pull {
    pub number: u64,
    /// Its id in GitHub's GraphQL API, which alone marks it a draft or
    /// ready for review.
    #[serde(default)]
    pub node_id: String,
    pub title: String,
    pub body: String,
    /// Who opened it.
    pub author: String,
    pub draft: bool,
    pub open: bool,
    /// The revision of its head.
    pub head: String,
    /// The branch its head is on.
    pub head_ref: String,
    /// The repository its head is in, `owner/name`; another than the
    /// pull request's for a fork, and empty when that fork is gone.
    pub head_repo: String,
    /// The branch it is into.
    pub base: String,
    /// When GitHub last changed it: a push, a review, a comment, a draft
    /// or ready mark, an edit; never another system's check (136).
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Merged, which a closed pull request sluice landed is.
    #[serde(default)]
    pub merged: bool,
}

impl Pull {
    /// From a branch of `repo` itself, not a fork.
    pub fn from_repo(&self, repo: &Repo) -> bool {
        self.head_repo == repo.to_string()
    }
}

/// One review, as GitHub keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub user: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED` or `DISMISSED`.
    pub state: String,
    /// The revision it was given on.
    pub commit: String,
}

/// How many reviewers approve `head`: each reviewer's last review that
/// decides anything, approving and given on `head` itself. A review on an
/// earlier head does not count (C.7 `reviews`).
pub fn approvals(reviews: &[Review], head: &str) -> u32 {
    let mut last: Vec<(&str, &Review)> = Vec::new();
    for r in reviews.iter().filter(|r| r.state != "COMMENTED") {
        match last.iter_mut().find(|(u, _)| *u == r.user) {
            Some(slot) => slot.1 = r,
            None => last.push((&r.user, r)),
        }
    }
    last.iter()
        .filter(|(_, r)| r.state == "APPROVED" && r.commit == head)
        .count() as u32
}

/// Where a check another system posts stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    Pending,
    Passed,
    Failed,
}

/// A check run or a commit status on a revision, by its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub standing: Standing,
}

/// The state of sluice's check run, as it posts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting, or in the queue.
    Queued,
    /// Under test.
    InProgress,
    /// Failed or conflict.
    Failure,
    /// Landed.
    Success,
}

/// sluice's check run on a head: its state, its title and its summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub status: Status,
    pub title: String,
    pub summary: String,
    /// ADR 0044: the check's text, the submission's report; `None` posts
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// sluice's check run as found on a head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posted {
    pub id: u64,
    /// Its name: `sluice`, or `sluice / <check> (<system>)` (ADR 0044).
    #[serde(default = "sluice_check")]
    pub name: String,
    pub posting: Posting,
}

fn sluice_check() -> String {
    SLUICE_CHECK.into()
}

/// A comment on a pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    /// Where a person opens it.
    pub url: String,
}

/// A branch's open pull requests as their list gave them, and whether the
/// list changed since the host last read it: `false` only when GitHub
/// answered that what the host keeps of it stands (136).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub pulls: Vec<Pull>,
    pub changed: bool,
}

/// An open pull request read whole, as one query reads every one of a
/// branch's (136): its reviews that decide anything, the checks other
/// systems posted on its head, sluice's own check runs there and when it
/// was marked ready for review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub pull: Pull,
    /// Each reviewer's latest review that decides anything, which
    /// [`approvals`] counts as it counts every review.
    pub reviews: Vec<Review>,
    /// Other systems' check runs and commit statuses on its head, the
    /// latest of each name.
    pub checks: Vec<Check>,
    /// sluice's own check runs on its head, each by its name.
    pub posted: Vec<Posted>,
    /// When it was marked ready for review, oldest first, the last ten.
    pub readied: Vec<DateTime<Utc>>,
}

/// Each reviewer's latest review that decides anything, in the order
/// given: what GitHub's `latestOpinionatedReviews` holds of `reviews`,
/// oldest first.
pub fn latest_opinionated(reviews: &[Review]) -> Vec<Review> {
    let mut out: Vec<Review> = Vec::new();
    for r in reviews.iter().filter(|r| r.state != "COMMENTED") {
        out.retain(|o| o.user != r.user);
        out.push(r.clone());
    }
    out
}

/// ADR 0044: the name of sluice's check of `task` on `system`.
pub fn check_name(task: &str, system: Option<&str>) -> String {
    match system {
        Some(s) => format!("{SLUICE_CHECK} / {task} ({s})"),
        None => format!("{SLUICE_CHECK} / {task}"),
    }
}

pub trait Forge {
    /// Who GitHub names as the author of a pull request sluice opens: a
    /// request whose author it is, sluice opened. Read with `repo`'s token
    /// where nothing else tells it (record 0108).
    fn login(&self, repo: &Repo) -> Result<String>;

    /// Every open pull request of the repository, into any branch: one
    /// list for the repository, which every branch reads its own from
    /// (`one-forge-read-per-repository-per-interval@b3b6757cea2d`).
    fn open_pulls(&self, repo: &Repo) -> Result<Vec<Pull>>;

    /// The open pull requests into `base`, read from the repository's one
    /// list.
    fn pulls(&self, repo: &Repo, base: &str) -> Result<Vec<Pull>> {
        Ok(self
            .open_pulls(repo)?
            .into_iter()
            .filter(|p| p.base == base)
            .collect())
    }

    /// Every open pull request of the repository, and whether their list
    /// changed since the host last read it; a forge that cannot tell says
    /// it did.
    fn listed(&self, repo: &Repo) -> Result<Listed> {
        Ok(Listed {
            pulls: self.open_pulls(repo)?,
            changed: true,
        })
    }

    /// Every open pull request into `base` read whole, in one query where
    /// the forge answers one (136; rule core.32).
    fn requests(&self, repo: &Repo, base: &str) -> Result<Vec<Request>>;

    /// Pull request `number`, open or closed; `None` when there is none.
    fn pull(&self, repo: &Repo, number: u64) -> Result<Option<Pull>>;

    /// Every review of pull request `number`, oldest first.
    fn reviews(&self, repo: &Repo, number: u64) -> Result<Vec<Review>>;

    /// The check runs and commit statuses other systems posted on `rev`,
    /// the latest of each name.
    fn checks(&self, repo: &Repo, rev: &str) -> Result<Vec<Check>>;

    /// When pull request `number` was marked ready for review, oldest
    /// first.
    fn readied(&self, repo: &Repo, number: u64) -> Result<Vec<DateTime<Utc>>>;

    /// sluice's own check run named `sluice` on `rev`, if it posted one.
    fn posted(&self, repo: &Repo, rev: &str) -> Result<Option<Posted>> {
        Ok(self
            .posted_all(repo, rev)?
            .into_iter()
            .find(|p| p.name == SLUICE_CHECK))
    }

    /// Every check run sluice's app posted on `rev`, each by its name.
    fn posted_all(&self, repo: &Repo, rev: &str) -> Result<Vec<Posted>>;

    /// Post sluice's check run on `rev`: update `existing`, or add one;
    /// its id.
    fn post(
        &self,
        repo: &Repo,
        rev: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64> {
        self.post_named(repo, rev, SLUICE_CHECK, existing, posting)
    }

    /// Post sluice's check run named `name` on `rev`: update `existing`,
    /// or add one; its id.
    fn post_named(
        &self,
        repo: &Repo,
        rev: &str,
        name: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64>;

    /// The bodies of the comments on pull request `number`.
    fn comments(&self, repo: &Repo, number: u64) -> Result<Vec<String>> {
        Ok(self
            .comment_list(repo, number)?
            .into_iter()
            .map(|c| c.body)
            .collect())
    }

    /// The comments on pull request `number`, oldest first.
    fn comment_list(&self, repo: &Repo, number: u64) -> Result<Vec<Comment>>;

    /// Comment `body` on pull request `number`; the comment's id.
    fn comment(&self, repo: &Repo, number: u64, body: &str) -> Result<u64>;

    /// Replace comment `id`'s body.
    fn edit_comment(&self, repo: &Repo, id: u64, body: &str) -> Result<()>;

    /// Open a pull request from the branch `head_ref` of `repo` into
    /// `base`, a draft or ready for review.
    fn open(
        &self,
        repo: &Repo,
        base: &str,
        head_ref: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<Pull>;

    /// Replace pull request `number`'s body, and its title when `title`
    /// is given, in one write.
    fn edit(&self, repo: &Repo, number: u64, title: Option<&str>, body: &str) -> Result<()>;

    /// Mark pull request `number` a draft, or ready for review, which
    /// GitHub's timeline records.
    fn set_draft(&self, repo: &Repo, number: u64, draft: bool) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(user: &str, state: &str, commit: &str) -> Review {
        Review {
            user: user.into(),
            state: state.into(),
            commit: commit.into(),
        }
    }

    /// C.7 `reviews`: each reviewer's last say counts, on the head alone.
    #[test]
    fn approvals_count_on_the_head_alone() {
        let rs = [
            review("ann", "APPROVED", "old"),
            review("bob", "APPROVED", "head"),
            review("cat", "APPROVED", "head"),
            review("cat", "COMMENTED", "head"),
            review("dan", "APPROVED", "head"),
            review("dan", "CHANGES_REQUESTED", "head"),
        ];
        assert_eq!(approvals(&rs, "head"), 2);
        assert_eq!(approvals(&rs, "old"), 1);
        assert_eq!(approvals(&[], "head"), 0);
    }

    /// 136: each reviewer's latest review that decides anything counts as
    /// every review does.
    #[test]
    fn the_latest_opinionated_reviews_count_as_every_review() {
        let rs = [
            review("ann", "APPROVED", "old"),
            review("bob", "APPROVED", "head"),
            review("cat", "APPROVED", "head"),
            review("cat", "COMMENTED", "head"),
            review("dan", "APPROVED", "head"),
            review("dan", "CHANGES_REQUESTED", "head"),
            review("ann", "APPROVED", "head"),
            review("ann", "DISMISSED", "head"),
        ];
        let latest = latest_opinionated(&rs);
        assert_eq!(
            latest
                .iter()
                .map(|r| (r.user.as_str(), r.state.as_str()))
                .collect::<Vec<_>>(),
            [
                ("bob", "APPROVED"),
                ("cat", "APPROVED"),
                ("dan", "CHANGES_REQUESTED"),
                ("ann", "DISMISSED"),
            ]
        );
        for head in ["head", "old"] {
            assert_eq!(approvals(&latest, head), approvals(&rs, head), "{head}");
        }
    }
}
