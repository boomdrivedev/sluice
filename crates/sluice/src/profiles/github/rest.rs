//! [`Forge`] over GitHub's REST API, as the app with its installation
//! token: pull requests, their reviews, the checks on their heads and
//! their timelines read; sluice's check run and comments posted. A list
//! is read a hundred at a time until a page comes back short. A branch's
//! open pull requests are read whole by one GraphQL query, fifty at a
//! time, from GraphQL's own allowance (136). A pull request is marked a
//! draft or ready for review through GraphQL, the one API that can. A check run, a comment or a pull request whose making's
//! answer was not read is read back on GitHub before it is made again
//! (134; rule core.29).

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::app::{App, Bearer, Call, Repo};
use super::forge::{
    Check, Comment, Forge, Listed, Posted, Posting, Pull, Request, Review, Standing, Status,
    SLUICE_CHECK,
};

/// The most a page of a list holds.
const PAGE: usize = 100;

/// The query that reads a branch's open pull requests whole, fifty to a
/// page: about three points of GraphQL's allowance a page (136).
const REQUESTS: &str = "query($owner: String!, $name: String!, $base: String!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, baseRefName: $base, first: 50, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number id title body isDraft updatedAt headRefOid headRefName
        author { __typename login } headRepository { nameWithOwner } baseRefName
        latestOpinionatedReviews(first: 30) { nodes { author { login } state commit { oid } } }
        timelineItems(itemTypes: [READY_FOR_REVIEW_EVENT], last: 10) {
          nodes { ... on ReadyForReviewEvent { createdAt } }
        }
        commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
          __typename
          ... on CheckRun { databaseId name status conclusion title summary text
                            checkSuite { app { databaseId } } }
          ... on StatusContext { context state }
        } } } } } }
      }
    }
  }
}";

impl App {
    fn repo_path(repo: &Repo, rest: &str) -> String {
        format!("/repos/{}/{}{rest}", repo.owner, repo.name)
    }

    /// Every item of a list, page by page; `items` picks the list out of
    /// a page's JSON.
    fn all(
        &self,
        repo: &Repo,
        path: &str,
        items: fn(&Value) -> Option<&Vec<Value>>,
    ) -> Result<Vec<Value>> {
        Ok(self.pages(repo, path, items)?.0)
    }

    /// [`App::all`], and whether GitHub sent any page afresh rather than
    /// answering that what the host keeps of it stands (136).
    fn pages(
        &self,
        repo: &Repo,
        path: &str,
        items: fn(&Value) -> Option<&Vec<Value>>,
    ) -> Result<(Vec<Value>, bool)> {
        let sep = if path.contains('?') { '&' } else { '?' };
        let mut out = Vec::new();
        let mut changed = false;
        for page in 1.. {
            let (v, fresh) = self.get_fresh(
                &format!("{path}{sep}per_page={PAGE}&page={page}"),
                Bearer::Repo(repo),
            )?;
            changed |= fresh;
            let got = items(&v).cloned().unwrap_or_default();
            let n = got.len();
            out.extend(got);
            if n < PAGE {
                break;
            }
        }
        Ok((out, changed))
    }

    /// Make something on `repo` with a `POST` of `body` to `path`, read
    /// from GitHub's answer by `answer` (134; rule core.29). A call whose
    /// answer was not read is not made again until `held` reads whether
    /// GitHub holds what it made: what it holds is carried on, and what it
    /// lacks is made then, once more.
    fn make<T>(
        &self,
        repo: &Repo,
        path: &str,
        body: &str,
        answer: impl Fn(&Value) -> Result<T>,
        held: impl FnOnce() -> Result<Option<T>>,
    ) -> Result<T> {
        let post = || self.post_api(Call::Make, path, Bearer::Repo(repo), body);
        match post() {
            Ok(v) => answer(&v),
            Err(e) if sluice_seams::unanswered(&e) => {
                match held().with_context(|| format!("{e:#}; reading whether it was made"))? {
                    Some(made) => Ok(made),
                    None => answer(&post()?),
                }
            }
            Err(e) => Err(e),
        }
    }
}

/// The id GitHub gave what it just made, a check run or a comment.
fn made_id(v: &Value, what: &str) -> Result<u64> {
    v["id"]
        .as_u64()
        .ok_or_else(|| anyhow!("GitHub made a {what} and gave it no id"))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn pull(v: &Value) -> Result<Pull> {
    Ok(Pull {
        number: v["number"]
            .as_u64()
            .ok_or_else(|| anyhow!("a pull request with no number"))?,
        node_id: text(&v["node_id"]),
        title: text(&v["title"]),
        body: text(&v["body"]),
        author: text(&v["user"]["login"]),
        draft: v["draft"].as_bool().unwrap_or(false),
        open: v["state"] == "open",
        head: text(&v["head"]["sha"]),
        head_ref: text(&v["head"]["ref"]),
        head_repo: text(&v["head"]["repo"]["full_name"]),
        base: text(&v["base"]["ref"]),
        updated_at: v["updated_at"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc)),
        merged: v["merged_at"].is_string(),
    })
}

/// A check run's standing from its status and conclusion.
fn run_standing(v: &Value) -> Standing {
    if v["status"] != "completed" {
        return Standing::Pending;
    }
    match v["conclusion"].as_str() {
        Some("success" | "neutral" | "skipped") => Standing::Passed,
        _ => Standing::Failed,
    }
}

/// A commit status's standing from its state.
fn status_standing(state: &str) -> Standing {
    match state {
        "success" => Standing::Passed,
        // `expected`, GraphQL's: a required status not yet reported.
        "pending" | "expected" => Standing::Pending,
        _ => Standing::Failed,
    }
}

fn posting_json(name: &str, posting: &Posting) -> Value {
    let (status, conclusion) = match posting.status {
        Status::Queued => ("queued", None),
        Status::InProgress => ("in_progress", None),
        Status::Failure => ("completed", Some("failure")),
        Status::Success => ("completed", Some("success")),
    };
    // GitHub takes no output without a summary.
    let summary = if posting.summary.is_empty() {
        &posting.title
    } else {
        &posting.summary
    };
    let mut v = json!({
        "name": name,
        "status": status,
        "output": { "title": posting.title, "summary": summary },
    });
    if let Some(text) = &posting.text {
        v["output"]["text"] = json!(text);
    }
    if let Some(c) = conclusion {
        v["conclusion"] = json!(c);
    }
    v
}

/// The body of the REST request that opens a pull request, a draft or
/// ready for review.
fn open_request(base: &str, head_ref: &str, title: &str, body: &str, draft: bool) -> Value {
    json!({ "title": title, "head": head_ref, "base": base, "body": body, "draft": draft })
}

/// The body of the REST request that replaces a pull request's body, and
/// its title only when given: a field left out is left as it is.
fn edit_request(title: Option<&str>, body: &str) -> Value {
    let mut v = json!({ "body": body });
    if let Some(title) = title {
        v["title"] = json!(title);
    }
    v
}

/// The GraphQL request that marks the pull request `node_id` a draft, or
/// ready for review.
fn draft_request(node_id: &str, draft: bool) -> Value {
    let mutation = if draft {
        "convertPullRequestToDraft"
    } else {
        "markPullRequestReadyForReview"
    };
    json!({
        "query": format!(
            "mutation($id: ID!) {{ {mutation}(input: {{pullRequestId: $id}}) \
             {{ pullRequest {{ isDraft }} }} }}"
        ),
        "variables": { "id": node_id },
    })
}

/// A GraphQL reply, an error when it carries any: GraphQL answers 200 to
/// a request it refuses.
fn graphql_reply(v: &Value) -> Result<()> {
    match v["errors"].as_array() {
        Some(errors) if !errors.is_empty() => {
            let why: Vec<&str> = errors
                .iter()
                .map(|e| e["message"].as_str().unwrap_or("an error with no message"))
                .collect();
            bail!("POST /graphql: {}", why.join("; "))
        }
        _ => Ok(()),
    }
}

/// The body of the GraphQL request for the page of `base`'s open pull
/// requests after the cursor `after`.
fn requests_request(repo: &Repo, base: &str, after: Option<&str>) -> Value {
    json!({
        "query": REQUESTS,
        "variables": { "owner": repo.owner, "name": repo.name, "base": base, "after": after },
    })
}

/// A GraphQL enum's value as REST spells it: `IN_PROGRESS` as
/// `in_progress`.
fn lower(v: &Value) -> Value {
    match v.as_str() {
        Some(s) => json!(s.to_ascii_lowercase()),
        None => Value::Null,
    }
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// One pull request of the query's answer read whole, the check runs of
/// the app `app` told apart as sluice's own.
fn request(n: &Value, app: u64) -> Result<Request> {
    let number = n["number"]
        .as_u64()
        .ok_or_else(|| anyhow!("a pull request with no number"))?;
    // GraphQL names a bot by its login alone, REST with `[bot]`.
    let author = match (
        n["author"]["__typename"].as_str(),
        text(&n["author"]["login"]),
    ) {
        (Some("Bot"), login) => format!("{login}[bot]"),
        (_, login) => login,
    };
    let pull = Pull {
        number,
        node_id: text(&n["id"]),
        title: text(&n["title"]),
        body: text(&n["body"]),
        author,
        draft: n["isDraft"].as_bool().unwrap_or(false),
        open: true,
        head: text(&n["headRefOid"]),
        head_ref: text(&n["headRefName"]),
        head_repo: text(&n["headRepository"]["nameWithOwner"]),
        base: text(&n["baseRefName"]),
        updated_at: time(&n["updatedAt"]),
        merged: false,
    };
    let nodes = |v: &Value| v["nodes"].as_array().cloned().unwrap_or_default();
    let reviews = nodes(&n["latestOpinionatedReviews"])
        .iter()
        .map(|r| Review {
            user: text(&r["author"]["login"]),
            state: text(&r["state"]),
            commit: text(&r["commit"]["oid"]),
        })
        .collect();
    let readied = nodes(&n["timelineItems"])
        .iter()
        .filter_map(|e| time(&e["createdAt"]))
        .collect();
    let contexts = nodes(&n["commits"])
        .first()
        .map(|c| nodes(&c["commit"]["statusCheckRollup"]["contexts"]))
        .unwrap_or_default();
    // Each check run as REST gives it, the latest of each name: the one
    // GitHub made last, which has the greatest id.
    let mut runs: Vec<Value> = Vec::new();
    let mut statuses: Vec<Check> = Vec::new();
    for c in &contexts {
        match c["__typename"].as_str() {
            Some("CheckRun") => {
                let run = json!({
                    "id": c["databaseId"],
                    "name": c["name"],
                    "status": lower(&c["status"]),
                    "conclusion": lower(&c["conclusion"]),
                    "output": { "title": c["title"], "summary": c["summary"], "text": c["text"] },
                    "app": { "id": c["checkSuite"]["app"]["databaseId"] },
                });
                match runs
                    .iter_mut()
                    .find(|r| r["name"] == run["name"] && r["app"]["id"] == run["app"]["id"])
                {
                    Some(r) if r["id"].as_u64() < run["id"].as_u64() => *r = run,
                    Some(_) => {}
                    None => runs.push(run),
                }
            }
            Some("StatusContext") => statuses.push(Check {
                name: text(&c["context"]),
                standing: status_standing(&c["state"].as_str().unwrap_or_default().to_lowercase()),
            }),
            _ => {}
        }
    }
    let ours = |r: &&Value| r["app"]["id"].as_u64() == Some(app);
    let posted = runs
        .iter()
        .filter(ours)
        .filter(|r| {
            let n = r["name"].as_str().unwrap_or_default();
            n == SLUICE_CHECK || n.starts_with(&format!("{SLUICE_CHECK} / "))
        })
        .filter_map(posted)
        .collect();
    let mut checks: Vec<Check> = runs
        .iter()
        .filter(|r| !ours(r))
        .map(|r| Check {
            name: text(&r["name"]),
            standing: run_standing(r),
        })
        .collect();
    checks.extend(statuses);
    Ok(Request {
        pull,
        reviews,
        checks,
        posted,
        readied,
    })
}

/// A page of the query's answer: its pull requests read whole, and the
/// cursor of the next page when there is one. An answer with errors is
/// an error, though GraphQL answers it 200.
fn requests_page(v: &Value, app: u64) -> Result<(Vec<Request>, Option<String>)> {
    graphql_reply(v)?;
    let prs = &v["data"]["repository"]["pullRequests"];
    if !prs.is_object() {
        bail!("POST /graphql: GitHub answered no repository's pull requests");
    }
    let got = prs["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| request(n, app))
        .collect::<Result<Vec<_>>>()?;
    let next = match prs["pageInfo"]["hasNextPage"].as_bool() {
        Some(true) => Some(
            prs["pageInfo"]["endCursor"]
                .as_str()
                .ok_or_else(|| anyhow!("POST /graphql: a next page with no cursor"))?
                .to_string(),
        ),
        _ => None,
    };
    Ok((got, next))
}

fn posted(v: &Value) -> Option<Posted> {
    let status = match (v["status"].as_str(), v["conclusion"].as_str()) {
        (Some("completed"), Some("success")) => Status::Success,
        (Some("completed"), _) => Status::Failure,
        (Some("in_progress"), _) => Status::InProgress,
        _ => Status::Queued,
    };
    let title = text(&v["output"]["title"]);
    let summary = text(&v["output"]["summary"]);
    Some(Posted {
        id: v["id"].as_u64()?,
        name: text(&v["name"]),
        posting: Posting {
            text: v["output"]["text"].as_str().map(String::from),
            status,
            // Posted with the title for an empty summary: read back as
            // empty, so it reads the same as what is posted.
            summary: if summary == title {
                String::new()
            } else {
                summary
            },
            title,
        },
    })
}

impl Forge for App {
    /// The app's bot user, which GitHub records as the author of every
    /// pull request the app opens with its installation's token.
    fn login(&self, repo: &Repo) -> Result<String> {
        self.bot(repo)
    }

    fn open_pulls(&self, repo: &Repo) -> Result<Vec<Pull>> {
        Ok(self.listed(repo)?.pulls)
    }

    fn listed(&self, repo: &Repo) -> Result<Listed> {
        let path = Self::repo_path(repo, "/pulls?state=open");
        let (items, changed) = self.pages(repo, &path, Value::as_array)?;
        Ok(Listed {
            pulls: items.iter().map(pull).collect::<Result<_>>()?,
            changed,
        })
    }

    fn requests(&self, repo: &Repo, base: &str) -> Result<Vec<Request>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let v = self.post_api(
                Call::Read,
                "/graphql",
                Bearer::Repo(repo),
                &requests_request(repo, base, after.as_deref()).to_string(),
            )?;
            let (got, next) = requests_page(&v, self.id())?;
            out.extend(got);
            match next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
    }

    fn pull(&self, repo: &Repo, number: u64) -> Result<Option<Pull>> {
        match self.get_found(
            &Self::repo_path(repo, &format!("/pulls/{number}")),
            Bearer::Repo(repo),
        )? {
            None => Ok(None),
            Some(v) => Ok(Some(pull(&v)?)),
        }
    }

    fn reviews(&self, repo: &Repo, number: u64) -> Result<Vec<Review>> {
        let path = Self::repo_path(repo, &format!("/pulls/{number}/reviews"));
        Ok(self
            .all(repo, &path, Value::as_array)?
            .iter()
            .map(|r| Review {
                user: text(&r["user"]["login"]),
                state: text(&r["state"]),
                commit: text(&r["commit_id"]),
            })
            .collect())
    }

    fn checks(&self, repo: &Repo, rev: &str) -> Result<Vec<Check>> {
        let runs = self.all(
            repo,
            &Self::repo_path(repo, &format!("/commits/{rev}/check-runs")),
            |v| v["check_runs"].as_array(),
        )?;
        let mut out: Vec<Check> = runs
            .iter()
            .filter(|r| r["app"]["id"].as_u64() != Some(self.id()))
            .map(|r| Check {
                name: text(&r["name"]),
                standing: run_standing(r),
            })
            .collect();
        let combined = self.get(
            &Self::repo_path(repo, &format!("/commits/{rev}/status?per_page={PAGE}")),
            Bearer::Repo(repo),
        )?;
        for s in combined["statuses"].as_array().into_iter().flatten() {
            out.push(Check {
                name: text(&s["context"]),
                standing: status_standing(s["state"].as_str().unwrap_or_default()),
            });
        }
        Ok(out)
    }

    fn readied(&self, repo: &Repo, number: u64) -> Result<Vec<DateTime<Utc>>> {
        let path = Self::repo_path(repo, &format!("/issues/{number}/timeline"));
        Ok(self
            .all(repo, &path, Value::as_array)?
            .iter()
            .filter(|e| e["event"] == "ready_for_review")
            .filter_map(|e| DateTime::parse_from_rfc3339(e["created_at"].as_str()?).ok())
            .map(|t| t.with_timezone(&Utc))
            .collect())
    }

    fn posted_all(&self, repo: &Repo, rev: &str) -> Result<Vec<Posted>> {
        let path = Self::repo_path(
            repo,
            &format!("/commits/{rev}/check-runs?app_id={}", self.id()),
        );
        let runs = self.all(repo, &path, |v| v["check_runs"].as_array())?;
        Ok(runs
            .iter()
            .filter(|r| r["app"]["id"].as_u64() == Some(self.id()))
            .filter(|r| {
                let n = r["name"].as_str().unwrap_or_default();
                n == SLUICE_CHECK || n.starts_with(&format!("{SLUICE_CHECK} / "))
            })
            .filter_map(posted)
            .collect())
    }

    fn post_named(
        &self,
        repo: &Repo,
        rev: &str,
        name: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64> {
        let mut body = posting_json(name, posting);
        match existing {
            Some(id) => {
                self.patch_api(
                    &Self::repo_path(repo, &format!("/check-runs/{id}")),
                    Bearer::Repo(repo),
                    &body.to_string(),
                )?;
                Ok(id)
            }
            None => {
                body["head_sha"] = json!(rev);
                self.make(
                    repo,
                    &Self::repo_path(repo, "/check-runs"),
                    &body.to_string(),
                    |v| made_id(v, "check run"),
                    || {
                        let runs = self.posted_all(repo, rev)?;
                        Ok(runs.iter().find(|p| p.name == name).map(|p| p.id))
                    },
                )
            }
        }
    }

    fn comment_list(&self, repo: &Repo, number: u64) -> Result<Vec<Comment>> {
        let path = Self::repo_path(repo, &format!("/issues/{number}/comments"));
        Ok(self
            .all(repo, &path, Value::as_array)?
            .iter()
            .map(|c| Comment {
                id: c["id"].as_u64().unwrap_or_default(),
                body: text(&c["body"]),
                url: text(&c["html_url"]),
            })
            .collect())
    }

    fn edit_comment(&self, repo: &Repo, id: u64, body: &str) -> Result<()> {
        self.patch_api(
            &Self::repo_path(repo, &format!("/issues/comments/{id}")),
            Bearer::Repo(repo),
            &json!({ "body": body }).to_string(),
        )?;
        Ok(())
    }

    fn comment(&self, repo: &Repo, number: u64, body: &str) -> Result<u64> {
        self.make(
            repo,
            &Self::repo_path(repo, &format!("/issues/{number}/comments")),
            &json!({ "body": body }).to_string(),
            |v| made_id(v, "comment"),
            || {
                let comments = self.comment_list(repo, number)?;
                Ok(comments.iter().find(|c| c.body == body).map(|c| c.id))
            },
        )
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
        self.make(
            repo,
            &Self::repo_path(repo, "/pulls"),
            &open_request(base, head_ref, title, body, draft).to_string(),
            pull,
            || {
                let pulls = self.pulls(repo, base)?;
                Ok(pulls
                    .into_iter()
                    .find(|p| p.head_ref == head_ref && p.from_repo(repo)))
            },
        )
    }

    fn edit(&self, repo: &Repo, number: u64, title: Option<&str>, body: &str) -> Result<()> {
        self.patch_api(
            &Self::repo_path(repo, &format!("/pulls/{number}")),
            Bearer::Repo(repo),
            &edit_request(title, body).to_string(),
        )?;
        Ok(())
    }

    fn set_draft(&self, repo: &Repo, number: u64, draft: bool) -> Result<()> {
        let pull = self
            .pull(repo, number)?
            .ok_or_else(|| anyhow!("{repo} has no pull request {number}"))?;
        if pull.node_id.is_empty() {
            bail!("GitHub gave {repo}'s pull request {number} no node id");
        }
        // Marking a draft or ready sets a state.
        let v = self.post_api(
            Call::Set,
            "/graphql",
            Bearer::Repo(repo),
            &draft_request(&pull.node_id, draft).to_string(),
        )?;
        graphql_reply(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 136: a pull request is read with when GitHub last changed it, and
    /// whether it was merged.
    #[test]
    fn a_pull_request_reads_its_last_change_and_its_merge() {
        let v = json!({
            "number": 7, "node_id": "PR_kw7", "title": "T", "body": "B",
            "user": { "login": "ann" }, "draft": false, "state": "closed",
            "head": { "sha": "abc", "ref": "unit/a", "repo": { "full_name": "o/r" } },
            "base": { "ref": "main" },
            "updated_at": "2026-10-28T12:34:56Z", "merged_at": "2026-10-28T12:34:50Z",
        });
        let p = pull(&v).unwrap();
        assert_eq!(
            p.updated_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-28T12:34:56Z")
                    .unwrap()
                    .into()
            )
        );
        assert!(p.merged && !p.open);
        let open = pull(&json!({ "number": 8, "state": "open", "merged_at": null })).unwrap();
        assert_eq!((open.updated_at, open.merged), (None, false));
    }

    /// What sluice posts reads back the same, so a round that would post
    /// it again posts nothing.
    #[test]
    fn a_posting_reads_back_as_posted() {
        for (status, summary) in [
            (Status::Queued, "needs 1 review"),
            (Status::InProgress, ""),
            (Status::Failure, "failed: #gate"),
            (Status::Success, ""),
        ] {
            let p = Posting {
                status,
                title: "t".into(),
                summary: summary.into(),
                text: (status == Status::Failure).then_some("| a | b |".into()),
            };
            let mut v = posting_json(SLUICE_CHECK, &p);
            v["id"] = json!(3);
            assert_eq!(posted(&v).unwrap().posting, p);
        }
    }

    /// A draft and a ready mark are one GraphQL mutation each, keyed by
    /// the pull request's node id; an `errors` array is an error, though
    /// GraphQL answers it with 200.
    #[test]
    fn marking_a_draft_or_ready() {
        let draft = draft_request("PR_kw1", true);
        let q = draft["query"].as_str().unwrap();
        assert!(
            q.contains("convertPullRequestToDraft(input: {pullRequestId: $id})"),
            "{q}"
        );
        assert!(q.starts_with("mutation($id: ID!)"), "{q}");
        assert_eq!(draft["variables"]["id"], "PR_kw1");
        let ready = draft_request("PR_kw1", false);
        let q = ready["query"].as_str().unwrap();
        assert!(
            q.contains("markPullRequestReadyForReview(input: {pullRequestId: $id})"),
            "{q}"
        );
        assert!(!q.contains("convertPullRequestToDraft"), "{q}");

        let ok = json!({ "data": { "markPullRequestReadyForReview": { "pullRequest": { "isDraft": false } } } });
        assert!(graphql_reply(&ok).is_ok());
        let refused = json!({ "data": null, "errors": [
            { "message": "Resource not accessible by integration" },
            { "type": "FORBIDDEN" },
        ] });
        let e = graphql_reply(&refused).unwrap_err().to_string();
        assert!(
            e.contains("Resource not accessible by integration")
                && e.contains("an error with no message"),
            "{e}"
        );
        assert!(graphql_reply(&json!({ "errors": [] })).is_ok());
    }

    /// A pull request is opened a draft only when asked.
    #[test]
    fn opening_a_draft_or_ready() {
        let draft = open_request("main", "unit/a", "A", "body", true);
        assert_eq!(draft["draft"], true);
        assert_eq!(
            (
                &draft["base"],
                &draft["head"],
                &draft["title"],
                &draft["body"]
            ),
            (
                &json!("main"),
                &json!("unit/a"),
                &json!("A"),
                &json!("body")
            )
        );
        assert_eq!(
            open_request("main", "unit/a", "A", "", false)["draft"],
            false
        );
    }

    /// An edit carries the title only when given, so one that gives none
    /// leaves the title GitHub holds.
    #[test]
    fn editing_the_body_and_the_title() {
        assert_eq!(edit_request(None, "body"), json!({ "body": "body" }));
        assert_eq!(
            edit_request(Some("Batch review cards"), "body"),
            json!({ "body": "body", "title": "Batch review cards" })
        );
    }

    /// A recorded answer of GitHub's to the query, two pull requests, the
    /// first of a bot's, with a next page.
    fn recorded() -> Value {
        serde_json::from_str(
            r#"{ "data": { "repository": { "pullRequests": {
            "pageInfo": { "hasNextPage": true, "endCursor": "Y3Vyc29yOjI=" },
            "nodes": [
                {
                    "number": 7, "id": "PR_kw7", "title": "T", "body": "B", "isDraft": false,
                    "updatedAt": "2026-10-29T10:00:00Z", "headRefOid": "aaa",
                    "headRefName": "unit/a", "author": { "__typename": "Bot", "login": "sluice" },
                    "headRepository": { "nameWithOwner": "o/r" }, "baseRefName": "main",
                    "latestOpinionatedReviews": { "nodes": [
                        { "author": { "login": "ann" }, "state": "APPROVED", "commit": { "oid": "old" } },
                        { "author": { "login": "bob" }, "state": "APPROVED", "commit": { "oid": "aaa" } }
                    ] },
                    "timelineItems": { "nodes": [ { "createdAt": "2026-10-29T09:00:00Z" }, {} ] },
                    "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "contexts": { "nodes": [
                        { "__typename": "CheckRun", "databaseId": 11, "name": "qa/approved",
                          "status": "COMPLETED", "conclusion": "FAILURE", "title": null,
                          "summary": null, "text": null, "checkSuite": { "app": { "databaseId": 1 } } },
                        { "__typename": "CheckRun", "databaseId": 12, "name": "qa/approved",
                          "status": "COMPLETED", "conclusion": "SUCCESS", "title": null,
                          "summary": null, "text": null, "checkSuite": { "app": { "databaseId": 1 } } },
                        { "__typename": "CheckRun", "databaseId": 13, "name": "ci",
                          "status": "IN_PROGRESS", "conclusion": null, "title": null,
                          "summary": null, "text": null, "checkSuite": { "app": { "databaseId": 2 } } },
                        { "__typename": "CheckRun", "databaseId": 21, "name": "sluice",
                          "status": "QUEUED", "conclusion": null, "title": "waiting",
                          "summary": "waiting", "text": null,
                          "checkSuite": { "app": { "databaseId": 4242 } } },
                        { "__typename": "CheckRun", "databaseId": 22, "name": "sluice / a:test",
                          "status": "COMPLETED", "conclusion": "FAILURE", "title": "failed",
                          "summary": "failed: a:test", "text": "| a | b |",
                          "checkSuite": { "app": { "databaseId": 4242 } } },
                        { "__typename": "StatusContext", "context": "legacy", "state": "EXPECTED" }
                    ] } } } } ] }
                },
                {
                    "number": 8, "id": "PR_kw8", "title": "U", "body": "", "isDraft": true,
                    "updatedAt": "2026-10-29T10:05:00Z", "headRefOid": "bbb",
                    "headRefName": "unit/b", "author": { "__typename": "User", "login": "cat" },
                    "headRepository": null, "baseRefName": "main",
                    "latestOpinionatedReviews": { "nodes": [] },
                    "timelineItems": { "nodes": [] },
                    "commits": { "nodes": [ { "commit": { "statusCheckRollup": null } } ] }
                }
            ]
        } } } }"#,
        )
        .unwrap()
    }

    /// 136: the query's answer read as the reads per request read: a
    /// bot's login as REST gives it, the latest check run of each name
    /// other systems posted, sluice's own told apart by the app's id, a
    /// status's standing, the ready marks, and the reviews counted by
    /// `approvals` as REST's would be.
    #[test]
    fn a_recorded_answer_reads_as_requests() {
        let (rs, next) = requests_page(&recorded(), 4242).unwrap();
        assert_eq!(next.as_deref(), Some("Y3Vyc29yOjI="));
        assert_eq!(rs.len(), 2);
        let a = &rs[0];
        assert_eq!(
            (
                a.pull.number,
                a.pull.node_id.as_str(),
                a.pull.author.as_str()
            ),
            (7, "PR_kw7", "sluice[bot]")
        );
        assert!(a.pull.open && !a.pull.draft && !a.pull.merged);
        assert_eq!(
            (
                a.pull.head.as_str(),
                a.pull.head_ref.as_str(),
                a.pull.head_repo.as_str()
            ),
            ("aaa", "unit/a", "o/r")
        );
        assert_eq!(a.pull.updated_at, time(&json!("2026-10-29T10:00:00Z")));
        // REST lists every review; the query each reviewer's latest that
        // decides anything, which counts the same.
        let rest = [
            Review {
                user: "ann".into(),
                state: "APPROVED".into(),
                commit: "old".into(),
            },
            Review {
                user: "bob".into(),
                state: "COMMENTED".into(),
                commit: "aaa".into(),
            },
            Review {
                user: "bob".into(),
                state: "APPROVED".into(),
                commit: "aaa".into(),
            },
        ];
        assert_eq!(super::super::forge::approvals(&a.reviews, "aaa"), 1);
        assert_eq!(
            super::super::forge::approvals(&a.reviews, "aaa"),
            super::super::forge::approvals(&rest, "aaa")
        );
        assert_eq!(
            a.checks,
            [
                Check {
                    name: "qa/approved".into(),
                    standing: Standing::Passed
                },
                Check {
                    name: "ci".into(),
                    standing: Standing::Pending
                },
                Check {
                    name: "legacy".into(),
                    standing: Standing::Pending
                },
            ]
        );
        assert_eq!(a.posted.len(), 2);
        assert_eq!(
            (a.posted[0].id, a.posted[0].name.as_str()),
            (21, SLUICE_CHECK)
        );
        assert_eq!(
            a.posted[0].posting,
            Posting {
                status: Status::Queued,
                title: "waiting".into(),
                summary: String::new(),
                text: None,
            }
        );
        assert_eq!(a.posted[1].posting.status, Status::Failure);
        assert_eq!(a.posted[1].posting.text.as_deref(), Some("| a | b |"));
        assert_eq!(a.readied, [time(&json!("2026-10-29T09:00:00Z")).unwrap()]);
        let b = &rs[1];
        assert_eq!(
            (b.pull.author.as_str(), b.pull.head_repo.as_str()),
            ("cat", "")
        );
        assert!(b.pull.draft);
        assert!(b.reviews.is_empty() && b.checks.is_empty() && b.posted.is_empty());

        let mut last = recorded();
        last["data"]["repository"]["pullRequests"]["pageInfo"] =
            json!({ "hasNextPage": false, "endCursor": null });
        assert_eq!(requests_page(&last, 4242).unwrap().1, None);
        // Asked again for the next page by its cursor.
        let asked = requests_request(
            &Repo::from_url("https://github.com/o/r").unwrap(),
            "main",
            Some("Y3Vyc29yOjI="),
        );
        assert_eq!(
            asked["variables"],
            json!({ "owner": "o", "name": "r", "base": "main", "after": "Y3Vyc29yOjI=" })
        );
        assert!(asked["query"]
            .as_str()
            .unwrap()
            .contains("latestOpinionatedReviews"));
    }

    /// A query GitHub refuses, other than for its allowance, is an error
    /// in GitHub's words; one that names no repository is one too.
    #[test]
    fn a_refused_query_is_an_error() {
        let refused = json!({ "data": null, "errors": [
            { "type": "FORBIDDEN", "message": "Resource not accessible by integration" },
        ] });
        let e = requests_page(&refused, 4242).unwrap_err().to_string();
        assert!(e.contains("Resource not accessible by integration"), "{e}");
        let e = requests_page(&json!({ "data": { "repository": null } }), 4242)
            .unwrap_err()
            .to_string();
        assert!(e.contains("no repository's pull requests"), "{e}");
    }

    /// 134 as amended 2026-10-28: GitHub's `RATE_LIMITED` answer to the
    /// query is a `Limited` failure, until the reset GitHub named, and no
    /// second query is made.
    #[test]
    fn a_rate_limited_answer_is_limited() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        const KEY: &[u8] = include_bytes!("../../../tests/profiles/forge/test-key.pem");
        let reset = Utc::now() + chrono::TimeDelta::minutes(20);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let queries = Arc::new(AtomicUsize::new(0));
        let counted = queries.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let mut len = 0;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let mut headers = String::new();
                let answer = match path.as_str() {
                    "/repos/o/r/installation" => json!({ "id": 7, "permissions": {} }),
                    "/app/installations/7/access_tokens" => json!({
                        "token": "ghs_t", "expires_at": (Utc::now() + chrono::TimeDelta::hours(1)).to_rfc3339(),
                    }),
                    "/graphql" => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        headers = format!(
                            "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: {}\r\nx-ratelimit-resource: graphql\r\n",
                            reset.timestamp()
                        );
                        json!({ "data": null, "errors": [{
                            "type": "RATE_LIMITED",
                            "message": "API rate limit exceeded for installation ID 7.",
                        }] })
                    }
                    _ => json!({ "message": "Not Found" }),
                };
                let body = answer.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nConnection: close\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let policy = std::rc::Rc::new(crate::retry::Policy::round());
        let app = App::new(4242, KEY, &api, Box::new(Utc::now), policy).unwrap();
        let repo = Repo::from_url("https://github.com/o/r").unwrap();
        let e = app.requests(&repo, "main").unwrap_err();
        let l = crate::retry::limited(&e).unwrap_or_else(|| panic!("not limited: {e:#}"));
        assert_eq!(l.until.timestamp(), reset.timestamp());
        assert!(l.words.contains("API rate limit exceeded"), "{}", l.words);
        assert_eq!(queries.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn standings() {
        assert_eq!(
            run_standing(&json!({"status": "in_progress"})),
            Standing::Pending
        );
        assert_eq!(
            run_standing(&json!({"status": "completed", "conclusion": "neutral"})),
            Standing::Passed
        );
        assert_eq!(
            run_standing(&json!({"status": "completed", "conclusion": "cancelled"})),
            Standing::Failed
        );
        assert_eq!(status_standing("error"), Standing::Failed);
    }
}
