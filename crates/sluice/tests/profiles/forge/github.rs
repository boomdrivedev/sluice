//! A fake of GitHub's API for one repository, `o/r`, on a port the system
//! picks: the app, its installation and tokens, and the pull request calls
//! the app makes, answered from a [`FileForge`]; a request the file holds
//! as sluice's shows the app's bot user as its author. Given the bare
//! repository that stands for GitHub's git, it keeps each open pull
//! request's head at its branch there and writes `refs/pull/<n>/head`, as
//! GitHub does. It marks a pull request a draft or ready for review
//! through GraphQL, as GitHub alone does, unless told to refuse, and
//! answers GraphQL's query of a branch's open pull requests whole, fifty
//! to a page, unless told to refuse it. It answers an error for a pull
//! request's reviews, the query that reads them among them, or for the
//! list of pull requests, while told to fail them, and answers a read of a pull
//! request 503 as often as told to. It refuses a pull request whose body
//! is longer than GitHub holds with GitHub's `422`, and keeps nothing of
//! it. Told to, it troubles the next calls of a method and path: answers
//! them with a status, takes them and then answers a status, or cuts the
//! connection once it read them, taken or not, or answers them as GitHub
//! does when an allowance is spent, or as the last call of one. It gives
//! every answer to a `GET` an ETag, answers `304` to a `GET` whose
//! `If-None-Match` is the answer's own, and keeps each call's status.
//! Each test crate that includes it uses a part.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use sluice::profiles::github::fake::{FileForge, LOGIN};
use sluice::profiles::github::forge::{Forge, Posting, Pull, Request as Whole, Standing, Status};
use sluice::profiles::github::Repo;

use crate::support::http::{json, Answer, Fake, Request, CUT};

pub const APP: u64 = 4242;
/// The app's slug, which GitHub names its bot user after.
pub const SLUG: &str = "sluice";
pub const INSTALLATION: u64 = 7;
/// The pull request calls' prefix.
const REPO: &str = "/repos/o/r";
/// The characters GitHub holds in a pull request's body.
const BODY_LIMIT: usize = 65_536;
/// The installation token the fake sells: one no answer holds by chance.
pub const TOKEN: &str = "ghs_fakeInstallationToken0123456789";

pub fn repo() -> Repo {
    Repo::from_url("https://github.com/o/r.git").unwrap()
}

pub struct GitHub {
    pub forge: Arc<FileForge>,
    pub fake: Fake,
    /// While set, GraphQL refuses to mark a pull request a draft or ready.
    refuse: Arc<AtomicBool>,
    /// While set, GraphQL refuses the query of a branch's pull requests.
    refuse_query: Arc<AtomicBool>,
    /// The calls answered with an error.
    failing: Arc<Mutex<Failing>>,
    /// How many reads of a pull request from now are answered 503.
    unavailable: Arc<AtomicUsize>,
    /// The calls troubled from now.
    troubles: Arc<Mutex<Vec<Troubled>>>,
    /// Each call's method, path and the status it was answered.
    answered: Arc<Mutex<Vec<(String, String, u16)>>>,
}

/// What the fake does to a call it is told to trouble.
#[derive(Clone, Debug)]
pub enum Trouble {
    /// Answers it with this status, these headers and this JSON, taking
    /// nothing of it: an allowance spent.
    Limited {
        status: u16,
        headers: Vec<(String, String)>,
        body: Value,
    },
    /// Answers it as it would, with these headers beside: the last call
    /// of an allowance.
    With(Vec<(String, String)>),
    /// Answers it with this status, taking nothing of it.
    Status(u16),
    /// Takes it, then answers it with this status, as GitHub may.
    Taken(u16),
    /// Reads it, takes it when `taken`, and cuts the connection with no
    /// answer.
    Cut { taken: bool },
}

/// The next `left` calls of `method` to a path starting `path`, troubled
/// as `trouble` says.
struct Troubled {
    method: String,
    path: String,
    left: usize,
    trouble: Trouble,
}

/// The calls the fake answers with an error while a test asks it to.
#[derive(Default)]
struct Failing {
    /// The pull requests whose reviews it gives no list of.
    reviews: BTreeSet<u64>,
    /// Whether it gives no list of pull requests.
    pulls: bool,
}

impl GitHub {
    /// The fake API over the fake GitHub kept in `file`; `bare` is the
    /// repository GitHub's git is, if any.
    pub fn start(file: &Path, bare: Option<&Path>) -> GitHub {
        let forge = Arc::new(FileForge::new(file, &repo()));
        let held = forge.clone();
        let bare = bare.map(Path::to_path_buf);
        let refuse = Arc::new(AtomicBool::new(false));
        let refusing = refuse.clone();
        let refuse_query = Arc::new(AtomicBool::new(false));
        let refusing_query = refuse_query.clone();
        let failing = Arc::new(Mutex::new(Failing::default()));
        let fails = failing.clone();
        let unavailable = Arc::new(AtomicUsize::new(0));
        let left = unavailable.clone();
        let troubles: Arc<Mutex<Vec<Troubled>>> = Arc::default();
        let troubling = troubles.clone();
        let answered: Arc<Mutex<Vec<(String, String, u16)>>> = Arc::default();
        let answering = answered.clone();
        let respond = move |r: &Request| -> Answer {
            if let Some(b) = &bare {
                sync(&held, b);
            }
            let trouble = troubling.lock().unwrap().iter_mut().find_map(|t| {
                let hit = t.left > 0 && r.method == t.method && r.path.starts_with(&t.path);
                hit.then(|| {
                    t.left -= 1;
                    t.trouble.clone()
                })
            });
            let take = |r: &Request| {
                let _ = answer(&held, bare.as_deref(), &fails.lock().unwrap(), r);
            };
            let mut beside = Vec::new();
            match trouble {
                Some(Trouble::Limited {
                    status,
                    headers,
                    body,
                }) => {
                    let (code, mut h, body) = json(status, body);
                    h.extend(headers);
                    return (code, h, body);
                }
                Some(Trouble::With(headers)) => beside = headers,
                Some(Trouble::Status(status)) => return erring(status),
                Some(Trouble::Taken(status)) => {
                    take(r);
                    return erring(status);
                }
                Some(Trouble::Cut { taken }) => {
                    if taken {
                        take(r);
                    }
                    return (CUT, vec![], String::new());
                }
                None => {}
            }
            let a_pull = r.method == "GET"
                && r.path
                    .strip_prefix(&format!("{REPO}/pulls/"))
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if a_pull
                && left
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
            {
                return json(503, json!({ "message": "Service Unavailable" }));
            }
            let (status, mut headers, body) = if r.method == "POST" && r.path == "/graphql" {
                let f = fails.lock().unwrap();
                let refuse_query = refusing_query.load(Ordering::SeqCst) || !f.reviews.is_empty();
                graphql(&held, refusing.load(Ordering::SeqCst), refuse_query, r)
            } else {
                answer(&held, bare.as_deref(), &fails.lock().unwrap(), r)
            };
            headers.extend(beside);
            versioned(r, (status, headers, body))
        };
        let fake = Fake::start(move |r| {
            let a = respond(r);
            answering
                .lock()
                .unwrap()
                .push((r.method.clone(), r.path.clone(), a.0));
            a
        });
        GitHub {
            forge,
            fake,
            refuse,
            refuse_query,
            failing,
            unavailable,
            troubles,
            answered,
        }
    }

    /// How many calls of `method` to a path starting `path` the fake
    /// answered `status`.
    pub fn answered(&self, method: &str, path: &str, status: u16) -> usize {
        self.answered
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, p, s)| m == method && p.starts_with(path) && *s == status)
            .count()
    }

    /// Trouble the next `n` calls of `method` to a path starting `path` as
    /// `trouble` says.
    pub fn trouble(&self, method: &str, path: &str, n: usize, trouble: Trouble) {
        self.troubles.lock().unwrap().push(Troubled {
            method: method.into(),
            path: path.into(),
            left: n,
            trouble,
        });
    }

    /// Every trouble set is lifted: the fake answers as GitHub does.
    pub fn calm(&self) {
        self.troubles.lock().unwrap().clear();
    }

    /// How many calls of `method` to a path starting `path` the fake saw.
    pub fn calls(&self, method: &str, path: &str) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.method == method && r.path.starts_with(path))
            .count()
    }

    /// The methods of the calls to a path starting `path` the fake saw,
    /// in the order it saw them.
    pub fn methods(&self, path: &str) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|r| r.path.starts_with(path))
            .map(|r| r.method)
            .collect()
    }

    pub fn url(&self) -> &str {
        self.fake.url()
    }

    /// Refuse, or take again, a request to mark a pull request a draft or
    /// ready for review.
    pub fn refuse_drafts(&self, refuse: bool) {
        self.refuse.store(refuse, Ordering::SeqCst);
    }

    /// Refuse, or answer again, the query of a branch's pull requests
    /// whole, as GitHub refuses one it cannot answer.
    pub fn refuse_query(&self, refuse: bool) {
        self.refuse_query.store(refuse, Ordering::SeqCst);
    }

    /// How many queries of a branch's pull requests whole the fake saw.
    pub fn queries(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| {
                r.method == "POST" && r.path == "/graphql" && r.body.contains("pullRequests(")
            })
            .count()
    }

    /// Answer an error for pull request `n`'s reviews, or give them again:
    /// the query of every pull request's reviews fails while one fails.
    pub fn fail_reviews(&self, n: u64, fail: bool) {
        let mut f = self.failing.lock().unwrap();
        if fail {
            f.reviews.insert(n);
        } else {
            f.reviews.remove(&n);
        }
    }

    /// Answer an error for the list of pull requests, or give it again.
    pub fn fail_pulls(&self, fail: bool) {
        self.failing.lock().unwrap().pulls = fail;
    }

    /// Answer the next `n` reads of a pull request 503, as GitHub does
    /// while it cannot answer.
    pub fn unavailable(&self, n: usize) {
        self.unavailable.store(n, Ordering::SeqCst);
    }

    /// How many of those are still to be answered.
    pub fn unavailable_left(&self) -> usize {
        self.unavailable.load(Ordering::SeqCst)
    }
}

/// GitHub's GraphQL API, for the two mutations sluice makes, a pull
/// request, by its node id, marked a draft or ready for review, refused
/// while `refuse`; and for its one query, a branch's open pull requests
/// whole, refused while `refuse_query`. GraphQL answers a request it
/// refuses with 200 and `errors`.
fn graphql(forge: &FileForge, refuse: bool, refuse_query: bool, r: &Request) -> Answer {
    let body: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
    let query = body["query"].as_str().unwrap_or_default();
    let error = |kind: &str, message: &str| {
        json(
            200,
            json!({ "data": null, "errors": [{ "type": kind, "message": message }] }),
        )
    };
    if query.contains("pullRequests(") {
        if refuse_query {
            return error("FORBIDDEN", "Resource not accessible by integration");
        }
        return pull_requests(forge, &body["variables"]);
    }
    let (mutation, draft) = if query.contains("convertPullRequestToDraft") {
        ("convertPullRequestToDraft", true)
    } else if query.contains("markPullRequestReadyForReview") {
        ("markPullRequestReadyForReview", false)
    } else {
        return error("UNKNOWN", &format!("the fake answers no {query}"));
    };
    if refuse {
        return error("FORBIDDEN", "Resource not accessible by integration");
    }
    let id = body["variables"]["id"].as_str().unwrap_or_default();
    let Some(n) = forge
        .held()
        .unwrap()
        .pulls
        .iter()
        .find(|p| p.node_id == id)
        .map(|p| p.number)
    else {
        return error(
            "NOT_FOUND",
            &format!("Could not resolve to a node with the global id of '{id}'"),
        );
    };
    forge.set_draft(n, draft).unwrap();
    json(
        200,
        json!({ "data": { mutation: { "pullRequest": { "isDraft": draft } } } }),
    )
}

/// The page of the query's answer `variables` ask for: the open pull
/// requests into `base`, fifty from the one after the cursor `after`, each
/// whole as GitHub's GraphQL spells it.
fn pull_requests(forge: &FileForge, variables: &Value) -> Answer {
    const FIRST: usize = 50;
    let base = variables["base"].as_str().unwrap_or_default();
    let from: usize = variables["after"]
        .as_str()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let all = forge.requests(&repo(), base).unwrap();
    let page: Vec<Value> = all.iter().skip(from).take(FIRST).map(whole_json).collect();
    let next = from + page.len();
    json(
        200,
        json!({ "data": { "repository": { "pullRequests": {
            "pageInfo": { "hasNextPage": next < all.len(), "endCursor": next.to_string() },
            "nodes": page,
        } } } }),
    )
}

/// A pull request read whole, as a node of GraphQL's answer.
fn whole_json(q: &Whole) -> Value {
    let p = &q.pull;
    let author = if p.author == LOGIN {
        json!({ "__typename": "Bot", "login": SLUG })
    } else {
        json!({ "__typename": "User", "login": p.author })
    };
    let upper = |s: &str| json!(s.to_ascii_uppercase());
    let mut contexts: Vec<Value> = q
        .posted
        .iter()
        .map(|r| {
            let run = run_json(r.id, &r.name, &r.posting);
            json!({
                "__typename": "CheckRun", "databaseId": r.id, "name": r.name,
                "status": upper(run["status"].as_str().unwrap()),
                "conclusion": run["conclusion"].as_str().map(upper),
                "title": run["output"]["title"], "summary": run["output"]["summary"],
                "text": run["output"]["text"], "checkSuite": { "app": { "databaseId": APP } },
            })
        })
        .collect();
    for c in &q.checks {
        let (status, conclusion) = match c.standing {
            Standing::Pending => ("IN_PROGRESS", Value::Null),
            Standing::Passed => ("COMPLETED", json!("SUCCESS")),
            Standing::Failed => ("COMPLETED", json!("FAILURE")),
        };
        contexts.push(json!({
            "__typename": "CheckRun", "databaseId": 1, "name": c.name, "status": status,
            "conclusion": conclusion, "title": null, "summary": null, "text": null,
            "checkSuite": { "app": { "databaseId": 1 } },
        }));
    }
    json!({
        "number": p.number, "id": p.node_id, "title": p.title, "body": p.body,
        "isDraft": p.draft, "updatedAt": p.updated_at.map(|t| t.to_rfc3339()),
        "headRefOid": p.head, "headRefName": p.head_ref, "author": author,
        "headRepository": (!p.head_repo.is_empty()).then(|| json!({ "nameWithOwner": p.head_repo })),
        "baseRefName": p.base,
        "latestOpinionatedReviews": { "nodes": q.reviews.iter().map(|r| json!({
            "author": { "login": r.user }, "state": r.state, "commit": { "oid": r.commit },
        })).collect::<Vec<_>>() },
        "timelineItems": { "nodes": q.readied.iter().map(|t| json!({ "createdAt": t.to_rfc3339() })).collect::<Vec<_>>() },
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "contexts": { "nodes": contexts } } } } ] },
    })
}

/// `a`, GitHub's answer to `r`, with an ETag when it is a `GET`'s 200:
/// `304` with nothing when `r` sent that ETag as `If-None-Match`.
fn versioned(r: &Request, a: Answer) -> Answer {
    let (status, mut headers, body) = a;
    if r.method != "GET" || status != 200 {
        return (status, headers, body);
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut h);
    let etag = format!("\"{:016x}\"", h.finish());
    if r.header("If-None-Match") == Some(etag.as_str()) {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
        headers.push(("ETag".into(), etag));
        return (304, headers, String::new());
    }
    headers.push(("ETag".into(), etag));
    (status, headers, body)
}

/// GitHub's pull requests follow their branches.
fn sync(forge: &FileForge, bare: &Path) {
    let Ok(held) = forge.held() else { return };
    for p in held.pulls.iter().filter(|p| p.open && p.from_repo(&repo())) {
        let Some(head) = rev(bare, &format!("refs/heads/{}", p.head_ref)) else {
            continue;
        };
        if head != p.head {
            forge.push(p.number, &head).unwrap();
        }
        set_ref(bare, &format!("refs/pull/{}/head", p.number), &head);
    }
    // A pull request whose head is in its branch is merged.
    for p in held.pulls.iter().filter(|p| p.open && !p.head.is_empty()) {
        let into = format!("refs/heads/{}", p.base);
        let merged = crate::support::git_command()
            .arg("--git-dir")
            .arg(bare)
            .args(["merge-base", "--is-ancestor", &p.head, &into])
            .output()
            .unwrap()
            .status
            .success();
        if merged {
            forge.merge(p.number).unwrap();
        }
    }
}

pub fn rev(bare: &Path, name: &str) -> Option<String> {
    let out = crate::support::git_command()
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-parse", "--verify", "-q", name])
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Remove the ref `name` from `bare`.
pub fn delete_ref(bare: &Path, name: &str) {
    let out = crate::support::git_command()
        .arg("--git-dir")
        .arg(bare)
        .args(["update-ref", "-d", name])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn set_ref(bare: &Path, name: &str, to: &str) {
    let out = crate::support::git_command()
        .arg("--git-dir")
        .arg(bare)
        .args(["update-ref", name, to])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Who GitHub shows as a pull request's author: the app's bot user for one
/// the file holds as sluice's.
fn author(p: &Pull) -> String {
    if p.author == LOGIN {
        format!("{SLUG}[bot]")
    } else {
        p.author.clone()
    }
}

fn pull_json(p: &Pull) -> Value {
    json!({
        "number": p.number,
        "node_id": p.node_id,
        "title": p.title,
        "body": p.body,
        "user": { "login": author(p) },
        "draft": p.draft,
        "state": if p.open { "open" } else { "closed" },
        "head": { "sha": p.head, "ref": p.head_ref, "repo": { "full_name": p.head_repo } },
        "base": { "ref": p.base },
        "updated_at": p.updated_at.map(|t| t.to_rfc3339()),
        "merged_at": p.updated_at.filter(|_| p.merged).map(|t| t.to_rfc3339()),
    })
}

fn posting(body: &Value) -> Posting {
    let status = match (body["status"].as_str(), body["conclusion"].as_str()) {
        (Some("completed"), Some("success")) => Status::Success,
        (Some("completed"), _) => Status::Failure,
        (Some("in_progress"), _) => Status::InProgress,
        _ => Status::Queued,
    };
    let title = body["output"]["title"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let summary = body["output"]["summary"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Posting {
        status,
        summary: if summary == title {
            String::new()
        } else {
            summary
        },
        title,
        text: body["output"]["text"].as_str().map(String::from),
    }
}

fn run_json(id: u64, name: &str, p: &Posting) -> Value {
    let (status, conclusion) = match p.status {
        Status::Queued => ("queued", Value::Null),
        Status::InProgress => ("in_progress", Value::Null),
        Status::Failure => ("completed", json!("failure")),
        Status::Success => ("completed", json!("success")),
    };
    let summary = if p.summary.is_empty() {
        &p.title
    } else {
        &p.summary
    };
    json!({
        "id": id, "name": name, "status": status, "conclusion": conclusion,
        "output": { "title": p.title, "summary": summary, "text": p.text }, "app": { "id": APP },
    })
}

fn not_found() -> Answer {
    json(404, json!({ "message": "Not Found" }))
}

/// GitHub answering `status`, in its words.
fn erring(status: u16) -> Answer {
    let why = match status {
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        429 => "API rate limit exceeded",
        404 => "Not Found",
        _ => "Server Error",
    };
    json(status, json!({ "message": why }))
}

/// GitHub failing a call of its own.
fn server_error() -> Answer {
    json(500, json!({ "message": "Server Error" }))
}

/// GitHub refusing a pull request's body longer than it holds.
fn too_long() -> Answer {
    json(
        422,
        json!({
            "message": "Validation Failed",
            "errors": [{
                "resource": "PullRequest", "field": "body", "code": "custom",
                "message": format!("body is too long (maximum is {BODY_LIMIT} characters)"),
            }],
        }),
    )
}

fn answer(forge: &FileForge, bare: Option<&Path>, failing: &Failing, r: &Request) -> Answer {
    let (path, query) = r.path.split_once('?').unwrap_or((&r.path, ""));
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(str::to_string)
    };
    // Every list fits on its first page.
    if param("page").is_some_and(|p| p != "1") {
        return if path.ends_with("/check-runs") {
            json(200, json!({ "check_runs": [] }))
        } else {
            json(200, json!([]))
        };
    }
    let body: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
    let m = r.method.as_str();
    if path == "/app" {
        return json(200, json!({ "id": APP, "slug": SLUG }));
    }
    if path == format!("{REPO}/installation") {
        return json(
            200,
            json!({ "id": INSTALLATION, "app_id": APP,
                    "permissions": { "contents": "write", "checks": "write", "pull_requests": "write" } }),
        );
    }
    // What a token reaches, as GitHub answers its holder (record 0108).
    if path == "/installation/repositories" && m == "GET" {
        return json(
            200,
            json!({ "total_count": 1, "repository_selection": "selected",
                    "repositories": [{ "full_name": "o/r" }] }),
        );
    }
    if path == format!("/app/installations/{INSTALLATION}/access_tokens") {
        let ends = chrono::Utc::now() + chrono::TimeDelta::hours(1);
        return json(
            201,
            json!({ "token": TOKEN, "expires_at": ends.to_rfc3339() }),
        );
    }
    let Some(rest) = path.strip_prefix(REPO) else {
        return not_found();
    };
    let rest: Vec<&str> = rest.split('/').skip(1).collect();
    let repo = repo();
    match (m, rest.as_slice()) {
        ("GET", ["pulls"]) if failing.pulls => server_error(),
        ("GET", ["pulls", n, "reviews"])
            if n.parse().is_ok_and(|n| failing.reviews.contains(&n)) =>
        {
            server_error()
        }
        ("GET", ["pulls"]) => {
            let base = param("base").unwrap_or_default();
            let ps = forge.pulls(&repo, &base).unwrap();
            json(200, Value::Array(ps.iter().map(pull_json).collect()))
        }
        ("POST", ["pulls"]) | ("PATCH", ["pulls", _])
            if body["body"]
                .as_str()
                .is_some_and(|b| b.chars().count() > BODY_LIMIT) =>
        {
            too_long()
        }
        ("POST", ["pulls"]) => {
            let p = forge
                .open(
                    &repo,
                    body["base"].as_str().unwrap(),
                    body["head"].as_str().unwrap(),
                    body["title"].as_str().unwrap_or_default(),
                    body["body"].as_str().unwrap_or_default(),
                    body["draft"].as_bool().unwrap_or(false),
                )
                .unwrap();
            if let Some(b) = bare {
                sync(forge, b);
            }
            let p = forge.pull(&repo, p.number).unwrap().unwrap();
            json(201, pull_json(&p))
        }
        ("GET", ["pulls", n]) => match forge.pull(&repo, n.parse().unwrap()).unwrap() {
            Some(p) => json(200, pull_json(&p)),
            None => not_found(),
        },
        ("PATCH", ["pulls", n]) => {
            let n: u64 = n.parse().unwrap();
            forge
                .edit(
                    &repo,
                    n,
                    body["title"].as_str(),
                    body["body"].as_str().unwrap_or_default(),
                )
                .unwrap();
            json(200, pull_json(&forge.pull(&repo, n).unwrap().unwrap()))
        }
        ("GET", ["pulls", n, "reviews"]) => {
            let rs = forge.reviews(&repo, n.parse().unwrap()).unwrap();
            json(
                200,
                Value::Array(
                    rs.iter()
                        .map(|r| json!({ "user": { "login": r.user }, "state": r.state, "commit_id": r.commit }))
                        .collect(),
                ),
            )
        }
        ("GET", ["commits", sha, "check-runs"]) => {
            let mut runs: Vec<Value> = Vec::new();
            for p in forge.posted_all(&repo, sha).unwrap() {
                runs.push(run_json(p.id, &p.name, &p.posting));
            }
            if param("check_name").is_none() {
                for c in forge.checks(&repo, sha).unwrap() {
                    let (status, conclusion) = match c.standing {
                        Standing::Pending => ("in_progress", Value::Null),
                        Standing::Passed => ("completed", json!("success")),
                        Standing::Failed => ("completed", json!("failure")),
                    };
                    runs.push(json!({ "id": 1, "name": c.name, "status": status,
                                      "conclusion": conclusion, "app": { "id": 1 } }));
                }
            }
            json(
                200,
                json!({ "total_count": runs.len(), "check_runs": runs }),
            )
        }
        ("GET", ["commits", _, "status"]) => {
            json(200, json!({ "state": "pending", "statuses": [] }))
        }
        ("POST", ["check-runs"]) => {
            let id = forge
                .post_named(
                    &repo,
                    body["head_sha"].as_str().unwrap(),
                    body["name"].as_str().unwrap(),
                    None,
                    &posting(&body),
                )
                .unwrap();
            json(201, json!({ "id": id }))
        }
        ("PATCH", ["check-runs", id]) => {
            let id: u64 = id.parse().unwrap();
            let held = forge.held().unwrap();
            let Some((sha, name)) = held
                .posted
                .iter()
                .find_map(|(sha, ps)| ps.iter().find(|p| p.id == id).map(|p| (sha, &p.name)))
            else {
                return not_found();
            };
            let name = body["name"].as_str().unwrap_or(name);
            forge
                .post_named(&repo, sha, name, Some(id), &posting(&body))
                .unwrap();
            json(200, json!({}))
        }
        ("GET", ["issues", n, "timeline"]) => {
            let ts = forge.readied(&repo, n.parse().unwrap()).unwrap();
            json(
                200,
                Value::Array(
                    ts.iter()
                        .map(|t| json!({ "event": "ready_for_review", "created_at": t.to_rfc3339() }))
                        .collect(),
                ),
            )
        }
        ("GET", ["issues", n, "comments"]) => {
            let cs = forge.comment_list(&repo, n.parse().unwrap()).unwrap();
            json(
                200,
                Value::Array(
                    cs.iter()
                        .map(|c| json!({ "id": c.id, "body": c.body, "html_url": c.url }))
                        .collect(),
                ),
            )
        }
        ("POST", ["issues", n, "comments"]) => {
            let id = forge
                .comment(
                    &repo,
                    n.parse().unwrap(),
                    body["body"].as_str().unwrap_or_default(),
                )
                .unwrap();
            json(201, json!({ "id": id }))
        }
        ("PATCH", ["issues", "comments", id]) => {
            forge
                .edit_comment(
                    &repo,
                    id.parse().unwrap(),
                    body["body"].as_str().unwrap_or_default(),
                )
                .unwrap();
            json(200, json!({}))
        }
        _ => not_found(),
    }
}

/// Where a test keeps its fake GitHub's file.
pub fn file(dir: &Path) -> PathBuf {
    dir.join("github.json")
}
