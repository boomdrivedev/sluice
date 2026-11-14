//! sluice's identity on GitHub, against a fake of GitHub's API on a port
//! the system picks (C.4 pushed refs). The tests that need git are in
//! the check-tools tier.

#[path = "../support/mod.rs"]
mod support;

#[path = "forge/github.rs"]
mod github;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;
use sluice::identity::{CredentialCommand, Identity};
use sluice::profiles::github::check::{check, Finding};
use sluice::profiles::github::{App, Bearer, Repo};
use sluice::retry::Policy;
use support::http::{json, Answer, Fake, Request};

const KEY: &[u8] = include_bytes!("forge/test-key.pem");
const APP: u64 = 4242;
const INSTALLATION: u64 = 7;

fn repo() -> Repo {
    Repo::from_url("https://github.com/o/r.git").unwrap()
}

/// A clock the test moves.
#[derive(Clone)]
struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    fn new() -> Clock {
        Clock(Arc::new(Mutex::new(
            DateTime::parse_from_rfc3339("2026-10-14T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )))
    }

    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }

    fn set(&self, t: DateTime<Utc>) {
        *self.0.lock().unwrap() = t;
    }
}

/// What the fake's GitHub holds for the app: its installation's
/// permissions, the repositories a token it gives reaches, each ruleset's
/// bypass actors on `main`, the id it gives the app `sluice-test`, and
/// what it answers git's push to `o/r`.
#[derive(Clone)]
struct Forge {
    permissions: serde_json::Value,
    reaches: serde_json::Value,
    rulesets: Vec<(u64, serde_json::Value)>,
    app: u64,
    push: u16,
    clock: Clock,
}

impl Forge {
    fn new(clock: &Clock) -> Forge {
        Forge {
            permissions: json!({
                "contents": "write", "workflows": "write", "checks": "write",
                "pull_requests": "write", "metadata": "read"
            }),
            reaches: json!([{ "full_name": "o/r" }]),
            rulesets: vec![(
                11,
                json!([{ "actor_id": APP, "actor_type": "Integration", "bypass_mode": "always" }]),
            )],
            app: APP,
            push: 200,
            clock: clock.clone(),
        }
    }

    /// The fake API; each token it gives is `token-<n>`, good for an hour.
    fn start(self) -> Fake {
        let minted = Mutex::new(0);
        Fake::start(move |r: &Request| self.answer(r, &minted))
    }

    /// The fake API's answer to `r`, `minted` the tokens it gave so far.
    fn answer(&self, r: &Request, minted: &Mutex<usize>) -> Answer {
        let path = r.path.as_str();
        if path == "/repos/o/r/installation" && r.method == "GET" {
            return json(
                200,
                json!({ "id": INSTALLATION, "app_id": APP, "permissions": self.permissions }),
            );
        }
        if path == format!("/app/installations/{INSTALLATION}/access_tokens") && r.method == "POST"
        {
            let mut n = minted.lock().unwrap();
            *n += 1;
            let ends = self.clock.now() + TimeDelta::hours(1);
            return json(
                201,
                json!({
                    "token": format!("token-{n}"), "expires_at": ends.to_rfc3339(),
                    "permissions": { "contents": "write", "metadata": "read" },
                    "repository_selection": "selected", "repositories": self.reaches
                }),
            );
        }
        if path == "/app" && r.method == "GET" {
            return json(200, json!({ "id": APP, "slug": "sluice-test" }));
        }
        if path == "/apps/sluice-test" && r.method == "GET" {
            return json(200, json!({ "id": self.app, "slug": "sluice-test" }));
        }
        // git's first ask of a push: its answer to any token sent as git
        // sends one.
        if path == "/o/r.git/info/refs?service=git-receive-pack" && r.method == "GET" {
            use base64::Engine;
            let given = r
                .header("Authorization")
                .and_then(|a| a.strip_prefix("Basic "))
                .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            let status = if given.starts_with("x-access-token:") {
                self.push
            } else {
                401
            };
            return (status, vec![], String::new());
        }
        // Who a token is, as GitHub answers its holder.
        if path == "/graphql" && r.method == "POST" && r.body.contains("viewer") {
            return json(
                200,
                json!({ "data": { "viewer": { "login": "sluice-test[bot]" } } }),
            );
        }
        // What a token reaches, as GitHub answers its holder.
        if path.starts_with("/installation/repositories") && r.method == "GET" {
            let total = self.reaches.as_array().map_or(0, Vec::len);
            return json(
                200,
                json!({
                    "total_count": total, "repository_selection": "selected",
                    "repositories": self.reaches
                }),
            );
        }
        if path == "/repos/o/r/rules/branches/main" {
            let rules: Vec<_> = self
                .rulesets
                .iter()
                .map(|(id, _)| json!({ "type": "update", "ruleset_id": id }))
                .collect();
            return json(200, json!(rules));
        }
        // Who may bypass a ruleset is shown to its admins alone.
        for (id, actors) in &self.rulesets {
            if path.starts_with(&format!("/repos/o/r/rulesets/{id}")) {
                let mut set = json!({ "id": id, "name": format!("ruleset {id}") });
                if r.header("Authorization") == Some("Bearer operator") {
                    set["bypass_actors"] = actors.clone();
                }
                return json(200, set);
            }
        }
        json(404, json!({ "message": "Not Found" }))
    }
}

/// The retry policy, its pauses skipped.
fn policy() -> Rc<Policy> {
    Rc::new(Policy::command().recording())
}

fn app(fake: &Fake, clock: &Clock) -> App {
    let clock = clock.clone();
    App::new(
        APP,
        KEY,
        fake.url(),
        Box::new(move || clock.now()),
        policy(),
    )
    .unwrap()
}

/// A credential command of the test's own (record 0108): a script, mode
/// `0755`, written into the test's scratch directory, `here` naming that
/// directory, where the script writes what it was asked; `body` runs with
/// the operation as `$1`.
fn credential_script(scratch: &support::Scratch, body: &str) -> CredentialCommand {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = scratch.path().join("token-helper");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&path)
        .unwrap();
    write!(f, "#!/bin/sh\nhere={}\n{body}", scratch.path().display()).unwrap();
    f.sync_all().unwrap();
    drop(f);
    CredentialCommand::new(path, vec![])
}

/// A script's body that answers `get` with `token-<n>`, `n` counting its
/// runs in `runs`, logging each operation in `log`, and an erase's
/// password beside it; `end` is the shell that prints the token's end
/// line, or nothing.
fn counting(end: &str) -> String {
    format!(
        "input=$(cat)\n\
         case \"$1\" in\n\
         get)\n\
           n=$(( $(cat \"$here/runs\" 2>/dev/null || echo 0) + 1 ))\n\
           echo $n > \"$here/runs\"\n\
           echo get >> \"$here/log\"\n\
           echo username=x-access-token\n\
           echo \"password=token-$n\"\n\
           {end}\n\
           ;;\n\
         erase)\n\
           echo \"erase $(echo \"$input\" | sed -n 's/^password=//p')\" >> \"$here/log\"\n\
           ;;\n\
         esac\n"
    )
}

/// How many times a counting script gave a token.
fn runs(scratch: &support::Scratch) -> usize {
    std::fs::read_to_string(scratch.path().join("runs")).map_or(0, |n| n.trim().parse().unwrap())
}

/// A counting script's log: each operation, an erase with its password.
fn script_log(scratch: &support::Scratch) -> Vec<String> {
    std::fs::read_to_string(scratch.path().join("log"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// The app whose tokens `command` gives, on the fake API, its clock `now`.
fn commanded(
    fake_url: &str,
    command: CredentialCommand,
    now: sluice::profiles::github::app::Now,
) -> App {
    App::commanded(APP, command, fake_url, now, policy())
}

fn minted(fake: &Fake) -> usize {
    fake.requests()
        .iter()
        .filter(|r| r.path.ends_with("/access_tokens"))
        .count()
}

/// The app's login is its bot user, named after the slug GitHub gives
/// the app's own JWT, read once however often it is asked for.
#[test]
fn the_apps_login_is_read_once() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let app = app(&fake, &clock);
    assert_eq!(app.login(&repo()).unwrap(), "sluice-test[bot]");
    assert_eq!(app.login(&repo()).unwrap(), "sluice-test[bot]");
    let reads: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path == "/app")
        .collect();
    assert_eq!(reads.len(), 1);
    let auth = reads[0].header("Authorization").unwrap_or_default();
    assert!(
        auth.starts_with("Bearer ") && !auth.starts_with("Bearer token-"),
        "{auth}"
    );
    assert_eq!(minted(&fake), 0);
}

/// The forge-identity spec's "The bot under a command": under a credential
/// command the app's bot is GraphQL's `viewer`, read once with the
/// command's token, and the app's own record, which takes its key, is
/// never asked (C.5 pull request, 120; record 0108).
#[test]
fn the_bot_under_a_command_is_the_viewers() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let scratch = support::Scratch::new("command-bot");
    let command = credential_script(&scratch, &counting(""));
    let app = commanded(fake.url(), command, ticking(&clock));
    assert_eq!(app.login(&repo()).unwrap(), "sluice-test[bot]");
    assert_eq!(app.login(&repo()).unwrap(), "sluice-test[bot]");
    let asked: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path == "/graphql")
        .map(|r| r.header("Authorization").unwrap_or_default().to_string())
        .collect();
    assert_eq!(asked, ["Bearer token-1"]);
    assert!(
        fake.requests().iter().all(|r| r.path != "/app"),
        "{:?}",
        fake.requests()
    );
}

/// A token is bought once and kept until five minutes before its hour
/// ends: over two hours, one is bought at 0, 56 and 112 minutes.
#[test]
fn a_token_is_bought_once_an_hour() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let app = app(&fake, &clock);
    let start = clock.now();
    let mut seen = Vec::new();
    for minute in 0..=120 {
        clock.set(start + TimeDelta::minutes(minute));
        let t = app.token(&repo()).unwrap();
        if seen.last() != Some(&t) {
            seen.push(t);
        }
    }
    assert_eq!(seen, ["token-1", "token-2", "token-3"]);
    assert_eq!(minted(&fake), 3);
    let bearer = fake.requests()[0]
        .header("Authorization")
        .unwrap()
        .to_string();
    assert!(bearer.starts_with("Bearer ey"), "{bearer}");
}

/// The forge-identity spec's "A token got once an hour": a command's token
/// is held until five minutes before the end the command gave, and got
/// again then; over two hours, at 0, 55 and 115 minutes, and nothing of
/// the app's key is asked (record 0108).
#[test]
fn a_commands_token_is_got_once_before_each_end() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let scratch = support::Scratch::new("command-hourly");
    let start = clock.now();
    std::fs::write(scratch.path().join("base"), start.timestamp().to_string()).unwrap();
    let command = credential_script(
        &scratch,
        &counting("echo \"password_expiry_utc=$(( $(cat \"$here/base\") + n * 3600 ))\""),
    );
    let at = clock.clone();
    let app = commanded(fake.url(), command, Box::new(move || at.now()));
    let mut seen = Vec::new();
    for minute in 0..=120 {
        clock.set(start + TimeDelta::minutes(minute));
        let t = app.token(&repo()).unwrap();
        if seen.last() != Some(&t) {
            seen.push(t);
        }
    }
    assert_eq!(seen, ["token-1", "token-2", "token-3"]);
    assert_eq!(runs(&scratch), 3);
    let asked: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert!(
        asked.iter().all(|p| !p.ends_with("/access_tokens")
            && p != "/app"
            && p != "/repos/o/r/installation"),
        "{asked:?}"
    );
}

/// The forge-identity spec's "A token with no end": a token the command
/// gives with no end is held until GitHub refuses it (record 0108).
#[test]
fn a_commands_token_with_no_end_is_held_until_refused() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let scratch = support::Scratch::new("command-no-end");
    let command = credential_script(&scratch, &counting(""));
    let at = clock.clone();
    let app = commanded(fake.url(), command, Box::new(move || at.now()));
    let start = clock.now();
    for minute in 0..=120 {
        clock.set(start + TimeDelta::minutes(minute));
        assert_eq!(app.token(&repo()).unwrap(), "token-1");
    }
    assert_eq!(runs(&scratch), 1);
}

/// The forge-identity spec's "A command's token described": the
/// credentials a command's token gives say whose it is, for what, from
/// which command, what GitHub says it reaches, its age and its end, read
/// once with the token, and never the token (134 as amended 2026-11-09).
#[test]
fn a_commands_credentials_say_what_their_token_is() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let scratch = support::Scratch::new("command-about");
    let start = clock.now();
    std::fs::write(scratch.path().join("base"), start.timestamp().to_string()).unwrap();
    let command = credential_script(
        &scratch,
        &counting("echo \"password_expiry_utc=$(( $(cat \"$here/base\") + 3600 ))\""),
    );
    let program = command.program().display().to_string();
    let app = commanded(fake.url(), command, ticking(&clock));
    let c = app.credentials("https://github.com/o/r.git").unwrap();
    assert_eq!(c.password, "token-1");
    assert!(
        c.about
            .starts_with("app 4242's token for o/r from [forge] credential "),
        "{}",
        c.about
    );
    assert!(c.about.contains(&program), "{}", c.about);
    assert!(c.about.contains("reaching o/r"), "{}", c.about);
    let end = (start + TimeDelta::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    assert!(c.about.ends_with(&format!("ending {end}")), "{}", c.about);
    assert!(!c.about.contains("token-1"), "{}", c.about);
    let reads: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path.starts_with("/installation/repositories"))
        .map(|r| r.header("Authorization").unwrap_or_default().to_string())
        .collect();
    assert_eq!(reads, ["Bearer token-1"]);
}

/// The forge-identity spec's "A token for another repository": a token
/// the command gives that GitHub says reaches another repository alone is
/// refused, naming the command and both, and nothing is called with it.
#[test]
fn a_commands_token_for_another_repository_is_refused() {
    let clock = Clock::new();
    let mut forge = Forge::new(&clock);
    forge.reaches = json!([{ "full_name": "o/other" }]);
    let fake = forge.start();
    let scratch = support::Scratch::new("command-other");
    let command = credential_script(&scratch, &counting(""));
    let program = command.program().display().to_string();
    let app = commanded(fake.url(), command, ticking(&clock));
    let err = format!("{:#}", app.token(&repo()).unwrap_err());
    assert!(err.contains(&program), "{err}");
    assert!(
        err.contains("gave a token for o/r that GitHub says reaches o/other, not o/r"),
        "{err}"
    );
    let asked: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| r.header("Authorization") == Some("Bearer token-1"))
        .map(|r| r.path)
        .collect();
    assert_eq!(asked, ["/installation/repositories?per_page=100"]);
}

/// A failure GitHub reports is an error that names it.
#[test]
fn a_refused_token_says_why() {
    let fake = Fake::start(|_| {
        json(
            401,
            json!({ "message": "A JSON web token could not be decoded" }),
        )
    });
    let app = app(&fake, &Clock::new());
    let err = format!("{:#}", app.token(&repo()).unwrap_err());
    assert!(
        err.contains("401 A JSON web token could not be decoded"),
        "{err}"
    );
}

/// A clock that moves ten seconds on at each reading.
fn ticking(clock: &Clock) -> sluice::profiles::github::app::Now {
    let clock = clock.clone();
    Box::new(move || {
        let t = clock.now();
        clock.set(t + TimeDelta::seconds(10));
        t
    })
}

/// The credentials for a remote on GitHub say what their token is: the
/// app, the repository, the installation it was bought from, the scope
/// GitHub gave it, and its age; renewed, they are a token bought then,
/// which the next call is given too.
#[test]
fn credentials_say_what_their_token_is_and_renew_buys_one() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let app = App::new(APP, KEY, fake.url(), ticking(&clock), policy()).unwrap();
    let url = "https://github.com/o/r.git";
    let c = app.credentials(url).unwrap();
    assert_eq!(c.password, "token-1");
    let scope = "scoped to o/r with contents: write, metadata: read";
    assert!(
        c.about
            .starts_with("app 4242's token for o/r from installation 7"),
        "{}",
        c.about
    );
    assert!(c.about.contains(scope), "{}", c.about);
    assert!(c.about.ends_with("s ago"), "{}", c.about);
    assert!(!c.about.contains("token-1"), "{}", c.about);
    assert_eq!(app.credentials(url).unwrap().password, "token-1");
    let fresh = app.renew(url).unwrap();
    assert_eq!(fresh.password, "token-2");
    assert!(fresh.about.contains(scope), "{}", fresh.about);
    assert_eq!(app.credentials(url).unwrap().password, "token-2");
    assert_eq!(minted(&fake), 2);
    let asked: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path.ends_with("/access_tokens"))
        .map(|r| serde_json::from_str::<serde_json::Value>(&r.body).unwrap())
        .collect();
    let alone = json!({ "repositories": ["r"] });
    assert_eq!(asked, [alone.clone(), alone], "for o/r alone");
}

/// A token GitHub gives that does not reach the repository is refused.
#[test]
fn a_token_for_another_repository_is_refused() {
    let clock = Clock::new();
    let mut forge = Forge::new(&clock);
    forge.reaches = json!([{ "full_name": "o/other" }]);
    let fake = forge.start();
    let err = format!("{:#}", app(&fake, &clock).token(&repo()).unwrap_err());
    assert!(
        err.contains("GitHub gave a token for o/r that reaches o/other, not o/r"),
        "{err}"
    );
}

/// deneb, 2026-11-05: GitHub finds no installation of the app on a
/// repository's old address once it is renamed. The credentials for it
/// are withheld in GitHub's words, with what to do beside them, so a read
/// that needed them is a refused read (133); the answer is asked once.
#[test]
fn no_installation_withholds_the_credentials_and_says_what_to_do() {
    let fake = Fake::start(|_| json(404, json!({ "message": "Not Found" })));
    let app = app(&fake, &Clock::new());
    let Err(e) = app.credentials("https://github.com/o/r.git") else {
        panic!("credentials given");
    };
    let w = sluice::identity::withheld(&e).expect("withheld");
    assert_eq!(w.said, "GET /repos/o/r/installation: 404 Not Found");
    let note = w.note.as_deref().unwrap_or_default();
    assert!(
        note.starts_with("GitHub finds no installation of app 4242 on o/r: if the repository was renamed or transferred, point the remote that names it at its new address"),
        "{note}"
    );
    assert_eq!(fake.requests().len(), 1);
}

/// GitHub answers a renamed or transferred repository's old address with a
/// redirect. The redirect is its answer, never followed, which would go
/// without the token, and says to point the remote at the new address.
#[test]
fn a_redirect_is_githubs_answer_and_says_what_to_do() {
    let clock = Clock::new();
    let forge = Forge::new(&clock);
    let minted = Mutex::new(0);
    let fake = Fake::start(move |r: &Request| {
        if !r.path.starts_with("/repos/o/r/pulls") {
            return forge.answer(r, &minted);
        }
        let moved = "https://api.github.com/repositories/42/pulls";
        let body = json!({ "message": "Moved Permanently", "url": moved });
        (
            301,
            vec![
                ("Location".into(), moved.into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body.to_string(),
        )
    });
    let app = app(&fake, &clock);
    let err = format!(
        "{:#}",
        app.get("/repos/o/r/pulls", Bearer::Repo(&repo()))
            .unwrap_err()
    );
    assert!(
        err.starts_with("GET /repos/o/r/pulls: 301 Moved Permanently\nGitHub answers o/r from another address, as it does once a repository is renamed or transferred: point the remote that names o/r at the repository's new address"),
        "{err}"
    );
    let asked: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        asked
            .iter()
            .filter(|p| p.starts_with("/repos/o/r/pulls"))
            .count(),
        1,
        "{asked:?}"
    );
    assert!(
        asked.iter().all(|p| !p.starts_with("/repositories/")),
        "{asked:?}"
    );
}

fn failures(found: &[Finding]) -> Vec<&str> {
    found
        .iter()
        .filter(|f| !f.ok)
        .map(|f| f.what.as_str())
        .collect()
}

/// All set: the app is installed to write contents, workflows, checks and
/// pull requests and is the only bypass of the branch's ruleset.
#[test]
fn forge_check_all_set() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let found = check(&app(&fake, &clock), &repo(), "main", Some("operator"));
    assert!(failures(&found).is_empty(), "{found:?}");
    assert_eq!(found.len(), 6, "{found:?}");
}

/// A ruleset that lets the operator's role through beside the app fails,
/// naming the extra bypass.
#[test]
fn forge_check_names_an_extra_bypass() {
    let clock = Clock::new();
    let mut forge = Forge::new(&clock);
    forge.rulesets[0].1 = json!([
        { "actor_id": APP, "actor_type": "Integration", "bypass_mode": "always" },
        { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" },
    ]);
    let fake = forge.start();
    let found = check(&app(&fake, &clock), &repo(), "main", Some("operator"));
    let f = failures(&found);
    assert_eq!(f.len(), 1, "{found:?}");
    assert!(
        f[0].contains("lets more than the app through: RepositoryRole 5"),
        "{}",
        f[0]
    );
}

/// Each other failure is named: a ruleset without the app, contents the
/// app may only read, workflows it may not write, no ruleset at all, and no token of the operator's
/// or one that cannot see the bypass list.
#[test]
fn forge_check_names_each_failure() {
    let clock = Clock::new();
    let one = |forge: Forge, operator: Option<&str>| -> Vec<String> {
        let fake = forge.start();
        let found = check(&app(&fake, &clock), &repo(), "main", operator);
        failures(&found).into_iter().map(String::from).collect()
    };

    let mut f = Forge::new(&clock);
    f.rulesets[0].1 =
        json!([{ "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }]);
    let got = one(f, Some("operator"));
    assert!(got[0].contains("does not let app 4242 through"), "{got:?}");

    let mut f = Forge::new(&clock);
    f.permissions = json!({
        "contents": "read", "workflows": "write", "checks": "write", "pull_requests": "write"
    });
    let got = one(f, Some("operator"));
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].contains("may not write o/r's contents (contents: read)"),
        "{got:?}"
    );

    let mut f = Forge::new(&clock);
    f.permissions = json!({ "contents": "write", "checks": "write", "pull_requests": "write" });
    let got = one(f, Some("operator"));
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].contains(
            "may not write o/r's workflows (workflows: none): give it Workflows read and write"
        ),
        "{got:?}"
    );

    // ADR 0044: no permission to post checks.
    let mut f = Forge::new(&clock);
    f.permissions = json!({
        "contents": "write", "workflows": "write", "checks": "read", "pull_requests": "write"
    });
    let got = one(f, Some("operator"));
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].contains("may not write o/r's checks (checks: read): give it Checks read and write"),
        "{got:?}"
    );

    let mut f = Forge::new(&clock);
    f.rulesets.clear();
    let got = one(f, Some("operator"));
    assert!(
        got[0].contains("no ruleset on GitHub protects main"),
        "{got:?}"
    );

    let got = one(Forge::new(&clock), None);
    assert!(got[0].contains("set GH_TOKEN"), "{got:?}");

    let got = one(Forge::new(&clock), Some("not-an-admin"));
    assert!(got[0].contains("cannot see who may bypass"), "{got:?}");

    let fake = Fake::start(|_| json(404, json!({ "message": "Not Found" })));
    let found = check(&app(&fake, &clock), &repo(), "main", Some("operator"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].what.contains("makes no token for o/r"),
        "{found:?}"
    );
}

/// The app on `fake` whose tokens a script the test writes gives, each
/// `token-<n>`, reaching the fake's git there too.
fn commanded_on(fake: &Fake, name: &str, clock: &Clock) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let command = credential_script(&scratch, &counting(""));
    let app = commanded(fake.url(), command, ticking(clock)).serving_git(fake.url());
    (scratch, app)
}

/// The forge-identity spec's "A command, all set": under a credential
/// command the check finds a token comes, it is the app's and may push,
/// and the ruleset lets only the app through, naming no key, installation
/// or permission (record 0108).
#[test]
fn forge_check_with_a_command_all_set() {
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let (_scratch, app) = commanded_on(&fake, "forge-check-command", &clock);
    let found = check(&app, &repo(), "main", Some("operator"));
    assert!(failures(&found).is_empty(), "{found:?}");
    let said: Vec<&str> = found.iter().map(|f| f.what.as_str()).collect();
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(
        said[0].starts_with("[forge] credential ") && said[0].ends_with(" gives a token for o/r"),
        "{said:?}"
    );
    assert_eq!(said[1], "the token is app 4242's");
    assert_eq!(said[2], "the token may push to o/r");
    assert!(said[3].contains("lets only app 4242 through"), "{said:?}");
    for word in ["key", "installation", "permission"] {
        assert!(said.iter().all(|s| !s.contains(word)), "{word}: {said:?}");
    }
}

/// The forge-identity spec's "A command that gives another app's token,
/// one that may not push": the check fails naming both apps, and says the
/// token may not push and the app needs Contents read and write (record
/// 0108).
#[test]
fn forge_check_with_a_command_names_each_failure() {
    let clock = Clock::new();
    let mut forge = Forge::new(&clock);
    forge.app = 99;
    forge.push = 403;
    let fake = forge.start();
    let (_scratch, app) = commanded_on(&fake, "forge-check-command-fails", &clock);
    let found = check(&app, &repo(), "main", Some("operator"));
    let f = failures(&found);
    assert_eq!(f.len(), 2, "{found:?}");
    assert!(
        f[0].contains("is app 99's (sluice-test), not app 4242's"),
        "{f:?}"
    );
    assert!(
        f[1].contains("the token may not push to o/r")
            && f[1].contains("give the app Contents read and write"),
        "{f:?}"
    );
}

/// A repository whose `main` lives on GitHub as configured, and on a bare
/// repository on this host as git reaches it; the host's config and the
/// fake API are the test's own.
#[cfg(feature = "tools")]
struct OnGitHub {
    repo: support::Repo,
    _bare: support::Scratch,
    config: support::Scratch,
    fake: Fake,
}

#[cfg(feature = "tools")]
impl OnGitHub {
    fn new(name: &str, forge: Forge) -> OnGitHub {
        let repo = support::Repo::new(name);
        repo.write(
            ".config/sluice.toml",
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n[branches.main]\nremote = \"origin\"\n",
        );
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        let bare = repo.bare_origin();
        repo.git(&["remote", "set-url", "origin", "https://github.com/o/r.git"]);
        repo.git(&[
            "config",
            &format!("url.{}.insteadOf", bare.path().display()),
            "https://github.com/o/r.git",
        ]);
        OnGitHub {
            repo,
            _bare: bare,
            config: support::Scratch::new(&format!("{name}-config")),
            fake: forge.start(),
        }
    }

    /// The host's `[forge]` names the app, its key with mode `0600`.
    fn give_forge(&self) {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.config.path().join("sluice");
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("app.pem");
        std::fs::write(&key, KEY).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            format!(
                "[forge]\napp_id = {APP}\nkey = {:?}\n",
                key.display().to_string()
            ),
        )
        .unwrap();
    }

    /// The host's `[forge]` names the app and, in place of its key, a
    /// credential command of the test's own: a script in the config
    /// directory that gives `token-1` (record 0108).
    fn give_forge_command(&self) {
        self.give_forge_command_running(
            "cat > /dev/null\n[ \"$1\" = get ] && echo password=token-1\nexit 0\n",
        );
    }

    /// The host's `[forge]` names the app and, in place of its key, a
    /// credential command of the test's own: a script in the config
    /// directory, mode `0755`, that runs `body` (record 0108).
    fn give_forge_command_running(&self, body: &str) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let dir = self.config.path().join("sluice");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("token-helper");
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&script)
            .unwrap();
        f.write_all(format!("#!/bin/sh\n{body}").as_bytes())
            .unwrap();
        f.sync_all().unwrap();
        drop(f);
        std::fs::write(
            dir.join("config.toml"),
            format!(
                "[forge]\napp_id = {APP}\ncredential = [{:?}]\n",
                script.display().to_string()
            ),
        )
        .unwrap();
    }

    /// `sluice <args>`: its exit and what it said. The operator's token is
    /// the fake's, so nothing asks gh for the user's.
    fn sluice(&self, args: &[&str]) -> (i32, String) {
        let out = support::isolated(&mut std::process::Command::new(env!(
            "CARGO_BIN_EXE_sluice"
        )))
        .current_dir(self.repo.dir())
        .args(args)
        .env("XDG_CONFIG_HOME", self.config.path())
        .env("XDG_STATE_HOME", self.config.path())
        .env("SLUICE_FORGE_API", self.fake.url())
        .env("GH_TOKEN", "operator")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GIT_DIR")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.code().unwrap_or(-1), text)
    }
}

/// `sluice status` on a branch whose `remote` is GitHub's: with no
/// `[forge]`, it says sluice has no identity of its own; with one, it
/// names the app. Neither fetches: a status reads the branch as the host
/// holds it since the host process last asked the remote, and asks GitHub
/// for no token of its own (129; design D16).
#[cfg(feature = "tools")]
#[test]
fn status_says_who_pushes_the_branch() {
    let s = OnGitHub::new("forge-status", Forge::new(&Clock::new()));
    let (code, without) = s.sluice(&["status", "main"]);
    assert_eq!(code, 0, "{without}");
    assert!(
        without.contains(
            "pushed to origin with this host's own git credentials: \
             sluice has no identity of its own there"
        ),
        "{without}"
    );
    assert_eq!(minted(&s.fake), 0);

    s.give_forge();
    let (code, with) = s.sluice(&["status", "main"]);
    assert_eq!(code, 0, "{with}");
    assert!(
        with.contains(&format!("pushed to origin as GitHub app {APP}")),
        "{with}"
    );
    assert_eq!(minted(&s.fake), 0, "a status fetches nothing of its own");
}

/// `sluice forge check main` exits 1 without `[forge]` or with a second
/// bypass, naming it, and 0 when all is set.
#[cfg(feature = "tools")]
#[test]
fn forge_check_exits_by_what_it_finds() {
    let clock = Clock::new();
    let s = OnGitHub::new("forge-check", Forge::new(&clock));
    let (code, said) = s.sluice(&["forge", "check", "main"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("no [forge] table"), "{said}");
    let (code, said) = s.sluice(&["forge", "check", "main", "--json"]);
    assert_eq!(code, 1, "{said}");
    let doc: serde_json::Value = serde_json::from_str(&said).unwrap();
    assert_eq!(
        (&doc["branch"], &doc["ok"]),
        (&json!("main"), &json!(false))
    );
    assert_eq!(doc["checks"][0]["ok"], false);
    assert!(
        doc["checks"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no [forge] table"),
        "{said}"
    );

    s.give_forge();
    let (code, said) = s.sluice(&["forge", "check"]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.contains(&format!("lets only app {APP} through")),
        "{said}"
    );
    let (code, said) = s.sluice(&["forge", "check", "--json"]);
    assert_eq!(code, 0, "{said}");
    let doc: serde_json::Value = serde_json::from_str(&said).unwrap();
    assert_eq!(doc["ok"], true);
    let checks = doc["checks"].as_array().unwrap();
    assert!(checks.iter().all(|c| c["ok"] == true), "{said}");
    assert!(
        checks.iter().any(|c| c["message"]
            .as_str()
            .unwrap()
            .contains(&format!("lets only app {APP} through"))),
        "{said}"
    );

    let mut extra = Forge::new(&clock);
    extra.rulesets[0].1 = json!([
        { "actor_id": APP, "actor_type": "Integration", "bypass_mode": "always" },
        { "actor_id": 1, "actor_type": "OrganizationAdmin", "bypass_mode": "always" },
    ]);
    let s = OnGitHub::new("forge-check-extra", extra);
    s.give_forge();
    let (code, said) = s.sluice(&["forge", "check", "main"]);
    assert_eq!(code, 1, "{said}");
    assert!(
        said.contains("lets more than the app through: OrganizationAdmin 1"),
        "{said}"
    );
}

/// The forge-identity spec's "A command, all set", from the command line:
/// `sluice forge check` on a host whose `[forge]` names a credential
/// command runs it, says it gives a token and the ruleset lets only the
/// app through, and says nothing of a key (record 0108).
#[cfg(feature = "tools")]
#[test]
fn forge_check_runs_the_hosts_command() {
    let s = OnGitHub::new("forge-check-command-host", Forge::new(&Clock::new()));
    s.give_forge_command();
    let (code, said) = s.sluice(&["forge", "check", "main"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("gives a token for o/r"), "{said}");
    assert!(
        said.contains(&format!("lets only app {APP} through")),
        "{said}"
    );
    assert!(!said.contains("[forge] key"), "{said}");
}

/// The forge-identity spec's "forge check tells the command's words
/// without its token": `sluice forge check` on a host whose credential
/// command refuses, naming a token on its standard error, says the
/// command's words with the token redacted, and nothing it says, as text
/// or as JSON, holds the token (record 0108 as amended 2026-11-10).
#[cfg(feature = "tools")]
#[test]
fn forge_check_tells_a_commands_words_without_its_token() {
    let s = OnGitHub::new("forge-check-command-words", Forge::new(&Clock::new()));
    s.give_forge_command_running(
        "cat > /dev/null\n\
         echo 'the cached ghs_0123456789abcdefghijABCDEFGHIJ012345 was refused' >&2\nexit 1\n",
    );
    let (code, said) = s.sluice(&["forge", "check", "main"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("the cached [redacted] was refused"), "{said}");
    assert!(!said.contains("0123456789abcdefghij"), "{said}");
    let (code, said) = s.sluice(&["forge", "check", "main", "--json"]);
    assert_eq!(code, 1, "{said}");
    assert!(!said.contains("0123456789abcdefghij"), "{said}");
}

/// The app's pull request calls against the fake API, answered from the
/// file-backed fake: what a person does there reads back through the app
/// as GitHub would show it, and what the app posts lands there.
#[test]
fn the_apps_pull_request_calls() {
    use sluice::profiles::github::forge::{approvals, Forge as _, Posting, Standing, Status};
    let dir = support::Scratch::new("forge-pulls");
    let gh = github::GitHub::start(&github::file(dir.path()), None);
    let app = App::new(APP, KEY, gh.url(), Box::new(Utc::now), policy()).unwrap();
    let f = &gh.forge;
    let r = repo();
    let a = f
        .open_by("ann", "main", "unit/a", "aaa", false, None)
        .unwrap();
    let d = f
        .open_by("dan", "main", "unit/d", "ddd", true, Some("dan/r"))
        .unwrap();
    f.open_by("cat", "bolt/x", "unit/c", "ccc", false, None)
        .unwrap();
    let pulls = app.pulls(&r, "main").unwrap();
    assert_eq!(pulls.iter().map(|p| p.number).collect::<Vec<_>>(), [a, d]);
    assert_eq!(pulls[0].author, "ann");
    assert_eq!(pulls[0].head, "aaa");
    assert!(pulls[0].from_repo(&r) && !pulls[1].from_repo(&r) && pulls[1].draft);
    assert_eq!(app.pull(&r, 99).unwrap(), None);

    f.review(a, "bob", "APPROVED", "old").unwrap();
    f.review(a, "cat", "APPROVED", "aaa").unwrap();
    assert_eq!(approvals(&app.reviews(&r, a).unwrap(), "aaa"), 1);
    f.check("aaa", "qa/approved", Standing::Passed).unwrap();
    f.check("aaa", "ci", Standing::Pending).unwrap();
    let checks = app.checks(&r, "aaa").unwrap();
    assert!(checks
        .iter()
        .any(|c| c.name == "qa/approved" && c.standing == Standing::Passed));
    assert!(checks
        .iter()
        .any(|c| c.name == "ci" && c.standing == Standing::Pending));
    f.set_draft(d, false).unwrap();
    assert_eq!(app.readied(&r, d).unwrap().len(), 1);

    // sluice's own check run, added then updated in place, and never
    // counted among other systems' checks.
    assert_eq!(app.posted(&r, "aaa").unwrap(), None);
    let waiting = Posting {
        status: Status::Queued,
        title: "waiting".into(),
        summary: "needs qa/approved".into(),
        text: None,
    };
    app.post(&r, "aaa", None, &waiting).unwrap();
    let first = app.posted(&r, "aaa").unwrap().unwrap();
    assert_eq!(first.posting, waiting);
    let landed = Posting {
        status: Status::Success,
        title: "landed".into(),
        summary: String::new(),
        text: Some("## sluice: landed\n| | Check |".into()),
    };
    app.post(&r, "aaa", Some(first.id), &landed).unwrap();
    assert_eq!(app.posted(&r, "aaa").unwrap().unwrap().posting, landed);
    assert_eq!(f.held().unwrap().posts.len(), 2);
    assert!(app
        .checks(&r, "aaa")
        .unwrap()
        .iter()
        .all(|c| c.name != "sluice"));

    app.comment(&r, a, "blamed").unwrap();
    assert_eq!(app.comments(&r, a).unwrap(), ["blamed"]);
    // ADR 0044: the check's text reads back, a check per check is sluice's
    // and no other system's, and a comment is edited in place.
    app.post_named(&r, "aaa", "sluice / a:test (linux-x64)", None, &waiting)
        .unwrap();
    let names: Vec<String> = app
        .posted_all(&r, "aaa")
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        names.contains(&"sluice / a:test (linux-x64)".to_string()),
        "{names:?}"
    );
    assert!(app
        .checks(&r, "aaa")
        .unwrap()
        .iter()
        .all(|c| !c.name.starts_with("sluice")));
    let c = app.comment_list(&r, a).unwrap().remove(0);
    assert!(
        c.url.contains(&format!("issuecomment-{}", c.id)),
        "{}",
        c.url
    );
    app.edit_comment(&r, c.id, "edited").unwrap();
    assert_eq!(app.comments(&r, a).unwrap(), ["edited"]);
    let opened = app.open(&r, "main", "unit/e", "E", "body", false).unwrap();
    assert!(opened.open && !opened.draft && opened.title == "E");
    let drafted = app.open(&r, "main", "unit/f", "F", "", true).unwrap();
    assert!(drafted.open && drafted.draft);
    app.edit(&r, opened.number, None, "new body").unwrap();
    let edited = app.pull(&r, opened.number).unwrap().unwrap();
    assert_eq!(
        (edited.title.as_str(), edited.body.as_str()),
        ("E", "new body")
    );
    app.edit(&r, opened.number, Some("E, retitled"), "new body")
        .unwrap();
    assert_eq!(
        app.pull(&r, opened.number).unwrap().unwrap().title,
        "E, retitled"
    );
}

/// The same calls against GitHub itself, as a scratch app on a scratch
/// repository: opt in with `SLUICE_GITHUB_TEST_REPO` (`owner/name`),
/// `SLUICE_GITHUB_TEST_APP` (the app's id), `SLUICE_GITHUB_TEST_KEY` (its
/// key) and `SLUICE_GITHUB_TEST_HEAD` (a branch of the repository with a
/// commit its default branch `SLUICE_GITHUB_TEST_BASE`, `main` if unset,
/// lacks). It opens a pull request from that branch, or uses the one
/// open, posts sluice's check run on its head twice and comments once.
/// Skipped when unset.
#[test]
fn live_pull_request_calls() {
    use sluice::profiles::github::forge::{Forge as _, Posting, Status};
    let Ok(name) = std::env::var("SLUICE_GITHUB_TEST_REPO") else {
        return;
    };
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is unset"));
    let r = Repo::from_url(&format!("https://github.com/{name}.git")).unwrap();
    let key = std::fs::read(var("SLUICE_GITHUB_TEST_KEY")).unwrap();
    let app = App::new(
        var("SLUICE_GITHUB_TEST_APP").parse().unwrap(),
        &key,
        sluice::profiles::github::app::API,
        Box::new(Utc::now),
        Rc::new(Policy::command()),
    )
    .unwrap();
    let base = std::env::var("SLUICE_GITHUB_TEST_BASE").unwrap_or_else(|_| "main".into());
    let head = var("SLUICE_GITHUB_TEST_HEAD");
    let pull = match app
        .pulls(&r, &base)
        .unwrap()
        .into_iter()
        .find(|p| p.head_ref == head)
    {
        Some(p) => p,
        None => app
            .open(
                &r,
                &base,
                &head,
                "sluice live test",
                "opened by sluice's live test",
                false,
            )
            .unwrap(),
    };
    app.reviews(&r, pull.number).unwrap();
    app.checks(&r, &pull.head).unwrap();
    app.readied(&r, pull.number).unwrap();
    let existing = app.posted(&r, &pull.head).unwrap().map(|p| p.id);
    let waiting = Posting {
        status: Status::Queued,
        title: "waiting".into(),
        summary: "sluice's live test".into(),
        text: None,
    };
    app.post(&r, &pull.head, existing, &waiting).unwrap();
    let posted = app.posted(&r, &pull.head).unwrap().unwrap();
    assert_eq!(posted.posting, waiting);
    let landed = Posting {
        status: Status::Success,
        title: "landed".into(),
        summary: String::new(),
        text: Some("## sluice: landed\n| | Check |".into()),
    };
    app.post(&r, &pull.head, Some(posted.id), &landed).unwrap();
    assert_eq!(app.posted(&r, &pull.head).unwrap().unwrap().posting, landed);
    app.comment(&r, pull.number, "sluice's live test").unwrap();
    assert!(app
        .comments(&r, pull.number)
        .unwrap()
        .iter()
        .any(|c| c == "sluice's live test"));
    app.edit(&r, pull.number, None, "edited by sluice's live test")
        .unwrap();
}

/// 88, 107, 111: GitHub Actions as a pool, against the fake API: a run is
/// one dispatch of the pool's workflow on the signer's branch, with the
/// runner of its system and its claim; a job is alive while a queued or
/// running run of the workflow bears its claim in its title; a wake is a
/// dispatch to advance the branch.
#[test]
fn the_pools_calls() {
    use sluice::profiles::checks::Launcher;
    use sluice::profiles::github::actions::Actions;
    use sluice_seams::{Launch, Rev, Setting};
    use std::collections::BTreeMap;

    let clock = Clock::new();
    let tokens = Forge::new(&clock);
    let fake = Fake::start(move |r: &Request| {
        let path = r.path.as_str();
        if path == "/repos/o/r/installation" || path.ends_with("/access_tokens") {
            return if path.ends_with("/access_tokens") {
                json(
                    201,
                    json!({ "token": "token-1", "expires_at": (tokens.clock.now() + TimeDelta::hours(1)).to_rfc3339() }),
                )
            } else {
                json(
                    200,
                    json!({ "id": INSTALLATION, "app_id": APP, "permissions": tokens.permissions }),
                )
            };
        }
        if path == "/repos/o/r/actions/workflows/sluice.yml/dispatches" && r.method == "POST" {
            return (204, vec![], String::new());
        }
        if path.starts_with("/repos/o/r/actions/workflows/sluice.yml/runs?status=in_progress") {
            return json(
                200,
                json!({ "workflow_runs": [{ "display_title": "sluice pool/forge/7" }] }),
            );
        }
        if path.starts_with("/repos/o/r/actions/workflows/sluice.yml/runs") {
            return json(200, json!({ "workflow_runs": [] }));
        }
        json(404, json!({ "message": "Not Found" }))
    });
    let dir = support::Scratch::new("pool-repo");
    for args in [
        &["init", "-q", "."][..],
        &["remote", "add", "origin", "https://github.com/o/r.git"][..],
    ] {
        let out = support::git_command()
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    let table = |pairs: &[(&str, &str)]| -> (String, Setting) {
        let t: BTreeMap<String, Setting> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Setting::Text(v.to_string())))
            .collect();
        ("github".into(), Setting::Tables(vec![t]))
    };
    let kind = table(&[("workflow", ".github/workflows/sluice.yml")]);
    let pool = Actions::new(Rc::new(app(&fake, &clock)), dir.path());
    pool.launch(&Launch {
        pool: "forge".into(),
        kind: kind.clone(),
        signer: table(&[
            ("workflow", ".github/workflows/sluice.yml"),
            ("ref", "refs/heads/main"),
        ]),
        runner: "macos-15".into(),
        remote: Some("origin".into()),
        branch: "main".into(),
        candidate: "c3".into(),
        combination: Rev::new("c0ffee"),
        system: Some("macos-arm64".into()),
        claim: "pool/forge/7".into(),
    })
    .unwrap();
    let sent = fake
        .requests()
        .into_iter()
        .find(|r| r.path.ends_with("/dispatches"))
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["ref"], "main");
    assert_eq!(body["inputs"]["mode"], "run");
    assert_eq!(body["inputs"]["runner"], "macos-15");
    assert_eq!(body["inputs"]["claim"], "pool/forge/7");
    assert_eq!(body["inputs"]["combination"], "c0ffee");
    assert!(pool
        .job_alive(&kind, "pool/forge/7", Some("origin"))
        .unwrap());
    assert!(!pool
        .job_alive(&kind, "pool/forge/8", Some("origin"))
        .unwrap());
    pool.wake(&kind, "main", Some("origin")).unwrap();
    let wake = fake
        .requests()
        .into_iter()
        .rfind(|r| r.path.ends_with("/dispatches"))
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&wake.body).unwrap();
    assert_eq!(body["inputs"]["mode"], "round");
    assert_eq!(body["inputs"]["branch"], "main");
}

// 134; rule core.29: every call to GitHub's API under the retry policy.

/// A policy whose pauses are kept and skipped, its clock moved on by each.
fn pausing() -> (Rc<Policy>, Rc<RefCell<Vec<Duration>>>) {
    let pauses = Rc::new(RefCell::new(Vec::new()));
    let clock = Rc::new(Cell::new(Instant::now()));
    let (kept, moved) = (pauses.clone(), clock.clone());
    let policy = Policy::command().pausing(
        move |d| {
            kept.borrow_mut().push(d);
            moved.set(moved.get() + d);
            true
        },
        move || clock.get(),
    );
    (Rc::new(policy), pauses)
}

/// The fake GitHub holding one open pull request from `unit/a` at `aaa`,
/// its number, and the app calling it under `policy`.
fn on_github(name: &str, policy: Rc<Policy>) -> (support::Scratch, github::GitHub, App, u64) {
    let dir = support::Scratch::new(name);
    let gh = github::GitHub::start(&github::file(dir.path()), None);
    let app = App::new(APP, KEY, gh.url(), Box::new(Utc::now), policy).unwrap();
    let n = gh
        .forge
        .open_by("ann", "main", "unit/a", "aaa", false, None)
        .unwrap();
    (dir, gh, app, n)
}

fn waiting() -> sluice::profiles::github::forge::Posting {
    use sluice::profiles::github::forge::{Posting, Status};
    Posting {
        status: Status::Queued,
        title: "waiting".into(),
        summary: "needs qa/approved".into(),
        text: None,
    }
}

/// The remote-retry spec's "A pull request read answered 503": a read
/// GitHub answers 503 three times is made a fourth, each after a longer
/// pause; one answered 404 is made once and reads as absent.
#[test]
fn a_read_github_could_not_answer_is_made_again() {
    use sluice::profiles::github::forge::Forge as _;
    let (policy, pauses) = pausing();
    let (_dir, gh, app, n) = on_github("forge-503", policy);
    gh.unavailable(3);
    let p = app.pull(&repo(), n).unwrap().unwrap();
    assert_eq!(p.number, n);
    assert_eq!(gh.calls("GET", &format!("/repos/o/r/pulls/{n}")), 4);
    let pauses = pauses.borrow().clone();
    assert_eq!(pauses.len(), 3, "{pauses:?}");
    assert!(pauses.windows(2).all(|w| w[1] > w[0]), "{pauses:?}");

    assert_eq!(app.pull(&repo(), 99).unwrap(), None);
    assert_eq!(gh.calls("GET", "/repos/o/r/pulls/99"), 1);
}

/// The remote-retry spec's "The policy's time is set by what called it":
/// a read GitHub answers 503 until the policy's thirty seconds are spent
/// fails as GitHub could not answer, saying how long it was tried.
#[test]
fn a_read_answered_503_until_the_bound_fails_unanswered() {
    use sluice::profiles::github::forge::Forge as _;
    let (policy, pauses) = pausing();
    let (_dir, gh, app, n) = on_github("forge-spent", policy);
    gh.unavailable(usize::MAX);
    let e = app.pull(&repo(), n).unwrap_err();
    assert!(sluice_seams::unanswered(&e), "{e:#}");
    let slept: Duration = pauses.borrow().iter().sum();
    assert_eq!(sluice::retry::lasted(&e), slept);
    assert!(
        slept > Duration::ZERO && slept <= Duration::from_secs(30),
        "{slept:?}"
    );
    let tries = pauses.borrow().len() + 1;
    assert_eq!(gh.calls("GET", &format!("/repos/o/r/pulls/{n}")), tries);
    let words = format!("{e:#}");
    assert!(
        words.contains(&format!("tried {tries} times over")),
        "{words}"
    );
    assert!(words.ends_with("503 Service Unavailable"), "{words}");
}

/// The remote-retry spec's "The forge's API refuses a token": a call
/// answered 401 on the token held is made again on one bought afresh,
/// which sluice holds after.
#[test]
fn a_call_refused_its_token_is_made_on_a_fresh_one() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let forge = Forge::new(&clock);
    let sold = Mutex::new(0);
    let fake = Fake::start(move |r: &Request| {
        if r.path != "/repos/o/r/pulls/7" {
            return forge.answer(r, &sold);
        }
        if r.header("Authorization") == Some("Bearer token-1") {
            return json(401, json!({ "message": "Bad credentials" }));
        }
        json(
            200,
            json!({ "number": 7, "state": "open", "head": { "ref": "unit/a" }, "base": { "ref": "main" } }),
        )
    });
    // A clock that moves on at each reading, so the fresh token is
    // settled at once.
    let app = App::new(APP, KEY, fake.url(), ticking(&clock), policy()).unwrap();
    assert_eq!(app.pull(&repo(), 7).unwrap().unwrap().number, 7);
    let asked: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path == "/repos/o/r/pulls/7")
        .map(|r| r.header("Authorization").unwrap().to_string())
        .collect();
    assert_eq!(asked, ["Bearer token-1", "Bearer token-2"]);
    assert_eq!(app.token(&repo()).unwrap(), "token-2");
    assert_eq!(minted(&fake), 2);
}

/// The fake API answering `/repos/o/r/pulls/7` with 401 to `refused` and
/// with the pull request to any other token; every other call as `forge`
/// answers it.
fn refusing(forge: Forge, refused: &'static str) -> Fake {
    let sold = Mutex::new(0);
    Fake::start(move |r: &Request| {
        if r.path != "/repos/o/r/pulls/7" {
            return forge.answer(r, &sold);
        }
        if r.header("Authorization") == Some(refused) {
            return json(401, json!({ "message": "Bad credentials" }));
        }
        json(
            200,
            json!({ "number": 7, "state": "open", "head": { "ref": "unit/a" }, "base": { "ref": "main" } }),
        )
    })
}

/// The `Authorization` each read of `/repos/o/r/pulls/7` carried.
fn pull_reads(fake: &Fake) -> Vec<String> {
    fake.requests()
        .into_iter()
        .filter(|r| r.path == "/repos/o/r/pulls/7")
        .map(|r| r.header("Authorization").unwrap().to_string())
        .collect()
}

/// The forge-identity spec's "Refused, erased and got afresh": a read
/// GitHub refuses on the command's token is made again on one the command
/// gives afresh, after it was told to erase the refused one, and sluice
/// holds the fresh one after (134; rule core.29; record 0108).
#[test]
fn a_call_refused_a_commands_token_is_made_on_one_got_afresh() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let fake = refusing(Forge::new(&clock), "Bearer token-1");
    let scratch = support::Scratch::new("command-renew");
    let command = credential_script(&scratch, &counting(""));
    // A clock that moves on at each reading, so the fresh token is
    // settled at once.
    let app = commanded(fake.url(), command, ticking(&clock));
    assert_eq!(app.pull(&repo(), 7).unwrap().unwrap().number, 7);
    assert_eq!(pull_reads(&fake), ["Bearer token-1", "Bearer token-2"]);
    assert_eq!(script_log(&scratch), ["get", "erase token-1", "get"]);
    assert_eq!(app.token(&repo()).unwrap(), "token-2");
}

/// The forge-identity spec's "The command gives the refused token back":
/// a command that gives again the token GitHub refused gives none afresh,
/// and the read is not made again (record 0108).
#[test]
fn a_command_that_gives_the_refused_token_back_gives_none_afresh() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let fake = refusing(Forge::new(&clock), "Bearer token-1");
    let scratch = support::Scratch::new("command-same");
    let command = credential_script(
        &scratch,
        "cat > /dev/null\n[ \"$1\" = get ] && echo password=token-1\nexit 0\n",
    );
    let app = commanded(fake.url(), command, ticking(&clock));
    let err = format!("{:#}", app.pull(&repo(), 7).unwrap_err());
    assert!(err.contains("401 Bad credentials"), "{err}");
    assert!(err.contains("no token could be got afresh"), "{err}");
    assert!(
        err.contains("gave again the token GitHub refused for o/r"),
        "{err}"
    );
    assert_eq!(pull_reads(&fake), ["Bearer token-1"]);
}

/// The forge-identity spec's "The command refuses", at the call: a command
/// that gives no token fails the call that needed it, in the command's own
/// words, and no call is made (record 0108).
#[test]
fn a_command_that_gives_no_token_fails_the_call() {
    use sluice::profiles::github::forge::Forge as _;
    let clock = Clock::new();
    let fake = Forge::new(&clock).start();
    let scratch = support::Scratch::new("command-refuses");
    let command = credential_script(
        &scratch,
        "cat > /dev/null\necho run >> \"$here/log\"\n\
         echo 'no drive on this host serves o/r' >&2\nexit 1\n",
    );
    let program = command.program().display().to_string();
    let app = commanded(fake.url(), command, ticking(&clock));
    let err = format!("{:#}", app.pull(&repo(), 7).unwrap_err());
    assert!(
        err.contains(&format!("[forge] credential {program}")),
        "{err}"
    );
    assert!(err.contains("no drive on this host serves o/r"), "{err}");
    assert_eq!(script_log(&scratch), ["run"]);
    assert!(fake.requests().is_empty(), "{:?}", fake.requests());
}

/// The remote-retry spec's "A draft mark answered 502" and "A check run's
/// update cut after it was sent": a write that sets a state is made again.
#[test]
fn a_write_that_sets_a_state_is_made_again() {
    use github::Trouble;
    use sluice::profiles::github::forge::{Forge as _, Posting, Status};
    let (_dir, gh, app, n) = on_github("forge-set", policy());
    let r = repo();
    gh.trouble("POST", "/graphql", 1, Trouble::Status(502));
    app.set_draft(&r, n, true).unwrap();
    assert_eq!(gh.calls("POST", "/graphql"), 2);
    assert!(app.pull(&r, n).unwrap().unwrap().draft);

    app.post(&r, "aaa", None, &waiting()).unwrap();
    let id = app.posted(&r, "aaa").unwrap().unwrap().id;
    let landed = Posting {
        status: Status::Success,
        title: "landed".into(),
        summary: String::new(),
        text: None,
    };
    gh.trouble(
        "PATCH",
        "/repos/o/r/check-runs/",
        1,
        Trouble::Cut { taken: false },
    );
    app.post(&r, "aaa", Some(id), &landed).unwrap();
    assert_eq!(gh.calls("PATCH", "/repos/o/r/check-runs/"), 2);
    assert_eq!(app.posted(&r, "aaa").unwrap().unwrap().posting, landed);
}

/// The remote-retry spec's "A comment posted, its answer lost", "A
/// comment not taken" and "A check run made, its answer lost": a write
/// that makes something whose answer was not read is read back on GitHub,
/// carried on when GitHub holds it and made once more when it does not.
#[test]
fn a_write_that_makes_something_is_read_back_before_it_is_made_again() {
    use github::Trouble;
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, gh, app, n) = on_github("forge-make", policy());
    let r = repo();
    let comments = format!("/repos/o/r/issues/{n}/comments");

    gh.trouble("POST", &comments, 1, Trouble::Taken(502));
    app.comment(&r, n, "taken").unwrap();
    assert_eq!(gh.calls("POST", &comments), 1);
    assert_eq!(app.comments(&r, n).unwrap(), ["taken"]);

    gh.trouble("POST", &comments, 1, Trouble::Status(502));
    app.comment(&r, n, "not taken").unwrap();
    assert_eq!(gh.calls("POST", &comments), 3);
    assert_eq!(app.comments(&r, n).unwrap(), ["taken", "not taken"]);

    gh.trouble(
        "POST",
        "/repos/o/r/check-runs",
        1,
        Trouble::Cut { taken: true },
    );
    app.post(&r, "aaa", None, &waiting()).unwrap();
    assert_eq!(gh.calls("POST", "/repos/o/r/check-runs"), 1);
    let names: Vec<String> = app
        .posted_all(&r, "aaa")
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["sluice"]);

    gh.trouble("POST", "/repos/o/r/pulls", 1, Trouble::Taken(502));
    let opened = app.open(&r, "main", "unit/e", "E", "body", false).unwrap();
    assert_eq!(
        (opened.head_ref.as_str(), opened.title.as_str()),
        ("unit/e", "E")
    );
    assert_eq!(gh.calls("POST", "/repos/o/r/pulls"), 1);
    assert_eq!(app.pulls(&r, "main").unwrap().len(), 2);
}

/// The remote-retry spec's "A connection never made": a comment whose
/// first try cannot connect is made again, and posted once.
#[test]
fn a_comment_whose_first_try_cannot_connect_is_posted_once() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, gh, app, n) = on_github("forge-unconnected", policy());
    let unreachable = Unreachable::default();
    let app = app.resolving(unreachable.clone());
    app.token(&repo()).unwrap();
    // The comment's first try is sent to port 0 of GitHub's host, where
    // no socket listens or can: its connection is never made, whatever
    // else the host runs.
    unreachable.next(1);
    app.comment(&repo(), n, "posted").unwrap();
    assert_eq!(unreachable.left(), 0, "the first try was never sent");
    // Never judged, the comment was made again blind: nothing read
    // whether GitHub held it before it was made, and it was made once
    // (core.29).
    let methods = gh.methods(&format!("/repos/o/r/issues/{n}/comments"));
    assert_eq!(methods, ["POST"]);
    assert_eq!(app.comments(&repo(), n).unwrap(), ["posted"]);
}

/// A lookup of each call's host that gives, for the next calls it is
/// told of, the host's addresses at port 0, which no socket can listen
/// on: a connection to them is refused, or the address is not one a
/// connection can be made to, and never reaches anything.
#[derive(Clone, Debug, Default)]
struct Unreachable(Arc<std::sync::atomic::AtomicUsize>);

impl Unreachable {
    /// Send the next `n` calls nowhere.
    fn next(&self, n: usize) {
        self.0.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// How many of them are still to be sent nowhere.
    fn left(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ureq::unversioned::resolver::Resolver for Unreachable {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        use std::sync::atomic::Ordering::SeqCst;
        let mut addrs = ureq::unversioned::resolver::DefaultResolver::default()
            .resolve(uri, config, timeout)?;
        if self
            .0
            .fetch_update(SeqCst, SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            addrs.iter_mut().for_each(|a| a.set_port(0));
        }
        Ok(addrs)
    }
}

/// The remote-retry spec's "A run's dispatch answered 502": a dispatch
/// GitHub answers 502 is made once, and fails saying its answer was not
/// read.
#[test]
fn a_dispatch_answered_502_is_made_once() {
    use sluice::profiles::checks::Launcher;
    use sluice::profiles::github::actions::Actions;
    use sluice_seams::{Launch, Rev, Setting};
    use std::collections::BTreeMap;

    let clock = Clock::new();
    let forge = Forge::new(&clock);
    let minted = Mutex::new(0);
    let fake = Fake::start(move |r: &Request| {
        if r.path.ends_with("/dispatches") {
            return json(502, json!({ "message": "Bad Gateway" }));
        }
        forge.answer(r, &minted)
    });
    let dir = support::Scratch::new("pool-unanswered");
    for args in [
        &["init", "-q", "."][..],
        &["remote", "add", "origin", "https://github.com/o/r.git"][..],
    ] {
        let out = support::git_command()
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    let workflow = |pairs: &[(&str, &str)]| -> (String, Setting) {
        let t: BTreeMap<String, Setting> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Setting::Text(v.to_string())))
            .collect();
        ("github".into(), Setting::Tables(vec![t]))
    };
    let kind = workflow(&[("workflow", ".github/workflows/sluice.yml")]);
    let pool = Actions::new(Rc::new(app(&fake, &clock)), dir.path());
    let e = pool
        .launch(&Launch {
            pool: "forge".into(),
            kind: kind.clone(),
            signer: kind,
            runner: "macos-15".into(),
            remote: Some("origin".into()),
            branch: "main".into(),
            candidate: "c3".into(),
            combination: Rev::new("c0ffee"),
            system: Some("macos-arm64".into()),
            claim: "pool/forge/7".into(),
        })
        .unwrap_err();
    assert!(sluice_seams::unanswered(&e), "{e:#}");
    let words = format!("{e:#}");
    assert!(
        words.contains(
            "GitHub's answer to POST /repos/o/r/actions/workflows/sluice.yml/dispatches \
             was not read: 502 Bad Gateway"
        ),
        "{words}"
    );
    let dispatched = fake
        .requests()
        .iter()
        .filter(|r| r.path.ends_with("/dispatches"))
        .count();
    assert_eq!(dispatched, 1);
}

// 134 as amended 2026-10-28, 136; rules core.29, core.32; ADR 0071: a
// spent allowance pauses every process on the host, and every read is
// conditional on the answer the host keeps.

/// `policy`, its pauses kept and skipped, its clock moved on by each.
fn pausing_as(policy: Policy) -> (Rc<Policy>, Rc<RefCell<Vec<Duration>>>) {
    let pauses = Rc::new(RefCell::new(Vec::new()));
    let clock = Rc::new(Cell::new(Instant::now()));
    let (kept, moved) = (pauses.clone(), clock.clone());
    let policy = policy.pausing(
        move |d| {
            kept.borrow_mut().push(d);
            moved.set(moved.get() + d);
            true
        },
        move || clock.get(),
    );
    (Rc::new(policy), pauses)
}

/// The app on `gh`, under `policy`, keeping its answers and pauses in
/// `state`, as each process on one host does.
fn kept_app(gh: &github::GitHub, state: &std::path::Path, policy: Rc<Policy>) -> App {
    App::new(APP, KEY, gh.url(), Box::new(Utc::now), policy)
        .unwrap()
        .keeping(sluice::profiles::github::kept::Kept::new(state))
}

/// The fake GitHub holding one open pull request, its number, and the
/// directory standing for the host's state.
fn kept_github(name: &str) -> (support::Scratch, support::Scratch, github::GitHub, u64) {
    let dir = support::Scratch::new(name);
    let state = support::Scratch::new(&format!("{name}-state"));
    let gh = github::GitHub::start(&github::file(dir.path()), None);
    let n = gh
        .forge
        .open_by("ann", "main", "unit/a", "aaa", false, None)
        .unwrap();
    (dir, state, gh, n)
}

fn headers(pairs: &[(&str, String)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// GitHub's 403 for the primary allowance spent until `reset`.
fn primary_spent(reset: DateTime<Utc>) -> github::Trouble {
    github::Trouble::Limited {
        status: 403,
        headers: headers(&[
            ("x-ratelimit-remaining", "0".into()),
            ("x-ratelimit-reset", reset.timestamp().to_string()),
            ("x-ratelimit-resource", "core".into()),
        ]),
        body: json!({ "message": "API rate limit exceeded for installation ID 7." }),
    }
}

/// Whether `d` is `want` to within the second GitHub's reset is given in.
fn about(d: Duration, want: Duration) -> bool {
    d <= want && want - d <= Duration::from_secs(1)
}

/// The forge-allowance spec's "The primary allowance spent", and the
/// remote-retry spec's "A spent allowance is no answer": a read answered
/// 403 with no call remaining is not made again before the reset, and a
/// waiting command reads again after it.
#[test]
fn a_spent_allowance_is_waited_out_to_its_reset() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("allowance-primary");
    let reset = Utc::now() + TimeDelta::minutes(20);
    let path = format!("/repos/o/r/pulls/{n}");
    gh.trouble("GET", &path, 1, primary_spent(reset));
    let (policy, pauses) = pausing_as(Policy::waiting(None));
    let app = kept_app(&gh, state.path(), policy);
    assert_eq!(app.pull(&repo(), n).unwrap().unwrap().number, n);
    assert_eq!(gh.answered("GET", &path, 403), 1);
    assert_eq!(gh.answered("GET", &path, 200), 1);
    let pauses = pauses.borrow().clone();
    assert_eq!(pauses.len(), 1, "the pause alone, no backoff: {pauses:?}");
    // Until the reset, which GitHub gives to the second.
    let twenty = Duration::from_secs(20 * 60);
    assert!(
        about(pauses[0], twenty) || about(pauses[0] + Duration::from_secs(1), twenty),
        "{pauses:?}"
    );
}

/// The forge-allowance spec's "One process meets it, another honours it":
/// a round refused with the allowance spent makes no call after it, and
/// another process on the host makes none before the reset either.
#[test]
fn every_process_on_the_host_honours_the_pause() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("allowance-host");
    let reset = Utc::now() + TimeDelta::minutes(20);
    gh.trouble("GET", "/repos/o/r/pulls", 1, primary_spent(reset));
    let run = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
    let e = run.pulls(&repo(), "main").unwrap_err();
    let limited = sluice::retry::limited(&e).expect("a spent allowance");
    assert_eq!(limited.until.timestamp(), reset.timestamp());
    assert!(
        limited
            .words
            .contains("API rate limit exceeded for installation ID 7."),
        "{e:#}"
    );
    let (policy, pauses) = pausing_as(Policy::round());
    let submit = kept_app(&gh, state.path(), policy);
    let e = submit.pull(&repo(), n).unwrap_err();
    assert!(sluice::retry::limited(&e).is_some(), "{e:#}");
    assert_eq!(gh.calls("GET", "/repos/o/r/pulls"), 1, "one call, refused");
    assert!(pauses.borrow().is_empty(), "a round waits no pause");
}

/// The forge-allowance spec's "A command's token honours the pause": a
/// process whose token the host's credential command gives makes no call
/// for a repository while a process on the app's key keeps the pause, and
/// the pause is kept under the app and the repository's owner (134;
/// record 0108).
#[test]
fn a_commands_token_honours_the_pause() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, _) = kept_github("allowance-command");
    let reset = Utc::now() + TimeDelta::minutes(20);
    gh.trouble("GET", "/repos/o/r/pulls", 1, primary_spent(reset));
    let run = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
    let e = run.pulls(&repo(), "main").unwrap_err();
    assert!(sluice::retry::limited(&e).is_some(), "{e:#}");
    let scratch = support::Scratch::new("allowance-command-script");
    let command = credential_script(
        &scratch,
        &format!("cat > /dev/null\necho password={}\n", github::TOKEN),
    );
    let submit = App::commanded(
        APP,
        command,
        gh.url(),
        Box::new(Utc::now),
        Rc::new(Policy::round().recording()),
    )
    .keeping(sluice::profiles::github::kept::Kept::new(state.path()));
    let e = submit.pulls(&repo(), "main").unwrap_err();
    let limited = sluice::retry::limited(&e).expect("a spent allowance");
    assert_eq!(limited.until.timestamp(), reset.timestamp());
    assert_eq!(gh.calls("GET", "/repos/o/r/pulls"), 1, "one call, refused");
    assert_eq!(gh.calls("GET", "/installation/repositories"), 0);
    assert!(state
        .path()
        .join(format!("forge/pauses/{APP}-o-core.json"))
        .is_file());
}

/// The forge-allowance spec's "The pause outlives the process": a process
/// started again before the pause ends makes no call before it ends, and
/// one waiting it out calls once it has.
#[test]
fn the_pause_outlives_the_process() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("allowance-restart");
    let reset = Utc::now() + TimeDelta::minutes(20);
    let path = format!("/repos/o/r/pulls/{n}");
    gh.trouble("GET", &path, 1, primary_spent(reset));
    {
        let first = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
        assert!(first.pull(&repo(), n).is_err());
    }
    let (policy, pauses) = pausing_as(Policy::command());
    let again = kept_app(&gh, state.path(), policy);
    assert_eq!(again.pull(&repo(), n).unwrap().unwrap().number, n);
    assert_eq!(
        gh.calls("GET", &path),
        2,
        "one refused, one after the pause"
    );
    assert_eq!(pauses.borrow().len(), 1);
}

/// The forge-allowance spec's "GraphQL's allowance spent" and "GraphQL's
/// allowance is its own": a query answered `RATE_LIMITED` pauses the
/// GraphQL allowance alone, and a paused REST allowance stops no query.
#[test]
fn graphqls_allowance_is_its_own() {
    use sluice::profiles::github::forge::Forge as _;
    use sluice::profiles::github::Bearer;
    let (_dir, state, gh, _) = kept_github("allowance-graphql");
    let reset = Utc::now() + TimeDelta::minutes(20);
    let query = json!({ "query": "query { viewer { login } }" }).to_string();
    let app = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
    // REST spent: the query is made all the same.
    gh.trouble("GET", "/repos/o/r/pulls", 1, primary_spent(reset));
    assert!(app.pulls(&repo(), "main").is_err());
    app.post_api(
        sluice::profiles::github::Call::Read,
        "/graphql",
        Bearer::Repo(&repo()),
        &query,
    )
    .unwrap();
    assert_eq!(gh.calls("POST", "/graphql"), 1);

    let (_dir, state, gh, n) = kept_github("allowance-graphql-spent");
    gh.trouble(
        "POST",
        "/graphql",
        1,
        github::Trouble::Limited {
            status: 200,
            headers: headers(&[
                ("x-ratelimit-remaining", "0".into()),
                ("x-ratelimit-reset", reset.timestamp().to_string()),
                ("x-ratelimit-resource", "graphql".into()),
            ]),
            body: json!({ "data": null, "errors": [{
                "type": "RATE_LIMITED", "message": "API rate limit exceeded for installation ID 7."
            }] }),
        },
    );
    let app = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
    for _ in 0..2 {
        let e = app
            .post_api(
                sluice::profiles::github::Call::Read,
                "/graphql",
                Bearer::Repo(&repo()),
                &query,
            )
            .unwrap_err();
        let l = sluice::retry::limited(&e).expect("limited");
        assert_eq!(l.until.timestamp(), reset.timestamp());
    }
    assert_eq!(gh.calls("POST", "/graphql"), 1, "no query before the reset");
    // REST is not paused by GraphQL's.
    assert!(app.pull(&repo(), n).unwrap().is_some());
}

/// The forge-allowance spec's "A secondary limit with retry-after", and
/// the remote-retry spec's "A 429 is no backoff": a 429 naming a time is
/// not called again before it, whatever the backoff's pauses would be.
#[test]
fn a_429_waits_as_long_as_github_says() {
    use sluice::profiles::github::forge::Forge as _;
    for secs in [90u64, 120] {
        let (_dir, state, gh, n) = kept_github(&format!("allowance-429-{secs}"));
        let path = format!("/repos/o/r/pulls/{n}");
        gh.trouble(
            "GET",
            &path,
            1,
            github::Trouble::Limited {
                status: 429,
                headers: headers(&[("retry-after", secs.to_string())]),
                body: json!({ "message": "You have exceeded a secondary rate limit." }),
            },
        );
        let (policy, pauses) = pausing_as(Policy::waiting(None));
        let app = kept_app(&gh, state.path(), policy);
        assert!(app.pull(&repo(), n).unwrap().is_some());
        assert_eq!(gh.calls("GET", &path), 2);
        assert_eq!(*pauses.borrow(), [Duration::from_secs(secs)]);
    }
}

/// The forge-allowance spec's "A secondary limit naming no time": the
/// first pause lasts a minute and the second, in a row, two.
#[test]
fn a_secondary_limit_naming_no_time_doubles() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("allowance-secondary");
    let path = format!("/repos/o/r/pulls/{n}");
    gh.trouble(
        "GET",
        &path,
        2,
        github::Trouble::Limited {
            status: 403,
            headers: vec![],
            body: json!({ "message": "You have exceeded a secondary rate limit. Please wait a few minutes before you try again." }),
        },
    );
    let (policy, pauses) = pausing_as(Policy::command());
    let app = kept_app(&gh, state.path(), policy);
    assert!(app.pull(&repo(), n).unwrap().is_some());
    assert_eq!(gh.calls("GET", &path), 3);
    assert_eq!(
        *pauses.borrow(),
        [Duration::from_secs(60), Duration::from_secs(120)]
    );
}

/// The forge-allowance spec's "The last call": a read that succeeds with
/// no call remaining is used, and the next call is not made before the
/// reset.
#[test]
fn the_last_call_of_an_allowance_starts_the_pause() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("allowance-last");
    let reset = Utc::now() + TimeDelta::minutes(20);
    gh.trouble(
        "GET",
        "/repos/o/r/pulls",
        1,
        github::Trouble::With(headers(&[
            ("x-ratelimit-remaining", "0".into()),
            ("x-ratelimit-reset", reset.timestamp().to_string()),
        ])),
    );
    let app = kept_app(&gh, state.path(), Rc::new(Policy::round().recording()));
    assert_eq!(app.pulls(&repo(), "main").unwrap().len(), 1);
    let e = app.pull(&repo(), n).unwrap_err();
    assert_eq!(
        sluice::retry::limited(&e).map(|l| l.until.timestamp()),
        Some(reset.timestamp())
    );
    assert_eq!(gh.calls("GET", &format!("/repos/o/r/pulls/{n}")), 0);
}

/// The forge-reads spec's "An unchanged list", "Another process on the
/// host" and "GitHub answers 200 to a kept ETag": every read carries the
/// ETag kept for its address, whichever process kept it; a 304 is
/// answered from what is kept, and a 200 replaces it.
#[test]
fn every_read_is_conditional() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, _) = kept_github("reads-conditional");
    let list = "/repos/o/r/pulls?state=open&base=main";
    let run = kept_app(&gh, state.path(), policy());
    let first = run.pulls(&repo(), "main").unwrap();
    let second = run.pulls(&repo(), "main").unwrap();
    assert_eq!(first, second);
    assert_eq!(gh.answered("GET", list, 200), 1);
    assert_eq!(gh.answered("GET", list, 304), 1);
    let sent: Vec<Option<String>> = gh
        .fake
        .requests()
        .iter()
        .filter(|r| r.path.starts_with(list))
        .map(|r| r.header("If-None-Match").map(String::from))
        .collect();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].is_none() && sent[1].is_some(), "{sent:?}");

    // Another process on the host carries the ETag the first kept.
    let wait = kept_app(&gh, state.path(), policy());
    assert_eq!(wait.pulls(&repo(), "main").unwrap(), first);
    let last = gh
        .fake
        .requests()
        .into_iter()
        .rfind(|r| r.path.starts_with(list))
        .unwrap();
    assert_eq!(last.header("If-None-Match").map(String::from), sent[1]);
    assert_eq!(gh.answered("GET", list, 304), 2);

    // Changed on GitHub: a 200 with a new ETag, used and kept.
    gh.forge
        .open_by("bob", "main", "unit/b", "bbb", false, None)
        .unwrap();
    assert_eq!(wait.pulls(&repo(), "main").unwrap().len(), 2);
    assert_eq!(gh.answered("GET", list, 200), 2);
    assert_eq!(run.pulls(&repo(), "main").unwrap().len(), 2);
    assert_eq!(gh.answered("GET", list, 304), 3);
}

/// The forge-reads spec's "No token on disk": after reading and posting
/// as the app, no file under the host's state holds its token.
#[test]
fn no_token_is_kept_on_the_host() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, n) = kept_github("reads-no-token");
    let app = kept_app(&gh, state.path(), policy());
    app.pulls(&repo(), "main").unwrap();
    app.comment(&repo(), n, "posted").unwrap();
    app.comments(&repo(), n).unwrap();
    assert_eq!(app.token(&repo()).unwrap(), github::TOKEN);
    let mut files = 0;
    let mut dirs = vec![state.path().to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                files += 1;
                let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
                assert!(!text.contains(github::TOKEN), "{}", p.display());
            }
        }
    }
    assert!(files > 0, "answers were kept");
}

// 136; rule core.32: one query reads a branch's open pull requests whole.

/// forge-reads, "Three pull requests, one query" and "Reviews counted as
/// before", as the app asks: one GraphQL query answers what the reads per
/// pull request do, each request's reviews counted by `approvals` as its
/// REST reviews are, other systems' checks and sluice's own check runs on
/// its head told apart, and its ready marks; and nothing by REST.
#[test]
fn one_query_reads_the_open_pull_requests_whole() {
    use sluice::profiles::github::forge::{approvals, Forge as _, Standing};
    let (_dir, gh, app, a) = on_github("query-whole", policy());
    let r = repo();
    let f = &gh.forge;
    let b = f
        .open_by("bob", "main", "unit/b", "bbb", true, None)
        .unwrap();
    let c = f
        .open_by("cat", "main", "unit/c", "ccc", false, Some("cat/r"))
        .unwrap();
    f.open_by("dan", "bolt/x", "unit/d", "ddd", false, None)
        .unwrap();
    f.set_draft(b, false).unwrap();
    f.review(a, "ann", "APPROVED", "old").unwrap();
    f.review(a, "bob", "APPROVED", "aaa").unwrap();
    f.review(a, "bob", "COMMENTED", "aaa").unwrap();
    f.check("aaa", "qa/approved", Standing::Passed).unwrap();
    f.check("aaa", "ci", Standing::Pending).unwrap();
    app.post(&r, "aaa", None, &waiting()).unwrap();
    let before = gh.fake.requests().len();

    let whole = app.requests(&r, "main").unwrap();
    let asked: Vec<String> = gh.fake.requests()[before..]
        .iter()
        .map(|q| format!("{} {}", q.method, q.path))
        .collect();
    assert_eq!(asked, ["POST /graphql"], "one query, nothing by REST");
    assert_eq!(gh.queries(), 1);
    assert_eq!(
        whole.iter().map(|q| q.pull.number).collect::<Vec<_>>(),
        [a, b, c]
    );
    let pulls = app.pulls(&r, "main").unwrap();
    for (q, p) in whole.iter().zip(&pulls) {
        assert_eq!(
            &q.pull,
            &sluice::profiles::github::forge::Pull {
                merged: false,
                ..p.clone()
            }
        );
    }
    let qa = &whole[0];
    assert_eq!(
        approvals(&qa.reviews, "aaa"),
        approvals(&app.reviews(&r, a).unwrap(), "aaa")
    );
    assert_eq!(approvals(&qa.reviews, "aaa"), 1);
    let mut checks = qa.checks.clone();
    let mut rest = app.checks(&r, "aaa").unwrap();
    checks.sort_by(|x, y| x.name.cmp(&y.name));
    rest.sort_by(|x, y| x.name.cmp(&y.name));
    assert_eq!(checks, rest);
    assert_eq!(qa.posted, app.posted_all(&r, "aaa").unwrap());
    assert_eq!(whole[1].readied, app.readied(&r, b).unwrap());
    assert_eq!(whole[1].readied.len(), 1);
    assert!(!whole[2].pull.from_repo(&r));
}

/// More open pull requests than a page holds are read fifty at a time,
/// each page after the last one's cursor.
#[test]
fn the_query_reads_fifty_to_a_page() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, gh, app, _) = on_github("query-pages", policy());
    for i in 0..50 {
        gh.forge
            .open_by("ann", "main", &format!("unit/p{i}"), "ppp", false, None)
            .unwrap();
    }
    let whole = app.requests(&repo(), "main").unwrap();
    assert_eq!(whole.len(), 51);
    assert_eq!(gh.queries(), 2);
    let afters: Vec<serde_json::Value> = gh
        .fake
        .requests()
        .iter()
        .filter(|q| q.path == "/graphql")
        .map(|q| {
            serde_json::from_str::<serde_json::Value>(&q.body).unwrap()["variables"]["after"]
                .clone()
        })
        .collect();
    assert_eq!(afters, [serde_json::Value::Null, json!("50")]);
}

/// forge-reads, "The query refused", as the app asks: GitHub's error
/// that is not `RATE_LIMITED` fails the query in GitHub's words, and is
/// no spent allowance.
#[test]
fn a_refused_query_fails_in_githubs_words() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, gh, app, _) = on_github("query-refused", policy());
    gh.refuse_query(true);
    let e = app.requests(&repo(), "main").unwrap_err();
    assert!(sluice::retry::limited(&e).is_none(), "{e:#}");
    assert!(
        format!("{e:#}").contains("Resource not accessible by integration"),
        "{e:#}"
    );
}

/// 136: the list says whether it changed since the host last read it:
/// not on GitHub's 304 for what the host keeps, and so on a 200.
#[test]
fn the_list_says_whether_it_changed() {
    use sluice::profiles::github::forge::Forge as _;
    let (_dir, state, gh, _) = kept_github("list-changed");
    let app = kept_app(&gh, state.path(), policy());
    assert!(app.listed(&repo(), "main").unwrap().changed);
    let again = app.listed(&repo(), "main").unwrap();
    assert!(!again.changed);
    assert_eq!(again.pulls.len(), 1);
    gh.forge
        .open_by("bob", "main", "unit/b", "bbb", false, None)
        .unwrap();
    let moved = app.listed(&repo(), "main").unwrap();
    assert!(moved.changed);
    assert_eq!(moved.pulls.len(), 2);
}
