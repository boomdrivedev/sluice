//! The app's JWT and its installation tokens. A JWT lasts ten minutes; an
//! installation token an hour, bought from the repository's own
//! installation for that repository alone, kept in this process's memory
//! and bought again five minutes before it ends, or at once for a call
//! GitHub refused it on. Nothing is written anywhere.
//!
//! The tokens are held in [`Tokens`], one for each app and repository,
//! which every app a process opens may share: sluice's host process holds
//! one for its life and hands it to every binding it makes, on every
//! thread, so a repository's token is got once for its hour however many
//! bindings read the repository, and one got while another call waits on
//! it is the one that call takes (record 0108 as amended 2026-11-19).
//!
//! Where the host names a credential command in place of the app's key,
//! the token for a repository is the command's, asked as git asks a
//! credential helper ([`CredentialCommand`]), held as a bought one is
//! until five minutes before the end the command gave, or until GitHub
//! refuses it where the command gave none; a refused one is erased and got
//! afresh from the command, and GitHub is asked once with each token got
//! which repositories it reaches, for what it is (134 as amended
//! 2026-11-09; record 0108).
//!
//! Every call to the API runs under the retry policy (134; rule core.29),
//! classed by what it does ([`Call`]): one GitHub never judged, a
//! connection not made, is tried again; a call GitHub refused the
//! installation's token on (401) is tried again on one bought afresh; a
//! read, or a write that sets a state, is tried again whatever else
//! failed but GitHub's answer; a write that makes something is not made
//! again once GitHub may have taken it, a 5xx among them, and fails
//! [`Unanswered`], for its caller to read whether it was made.
//!
//! GitHub's word that an allowance is spent, a 429, a 403 that says so,
//! GraphQL's `RATE_LIMITED`, or a success that leaves no call, is a pause
//! kept in the host's state for the installation and the allowance, read
//! before every call by every process on the host: while it stands no
//! call is made, and none to find out (134 as amended 2026-10-28; rule
//! core.29; ADR 0071). Every `GET` carries the ETag and last-modified time
//! of the answer the host keeps for its address, and GitHub's 304 is
//! answered from it (136; rule core.32). The calls the app's JWT makes,
//! finding the installation and buying a token, read no pause.
//!
//! A redirect is GitHub's answer, never followed: GitHub answers a renamed
//! or transferred repository's old address with one, and a call followed
//! there would go without its token. That answer, and no installation of
//! the app found on a repository, say to point the remote that names it
//! at the repository's new address, which the next call reads.
//!
//! The app's bot, the author of every commit sluice pushes as the app, is
//! read with the app's own credential and kept in the host's state for a
//! day, standing while GitHub cannot be read after it ([`App::author`];
//! record 0019 as amended 2026-11-18; rule core.43).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::Value;
use sluice_seams::Unanswered;

use super::client::{Budget, Client, Reroute};
use super::kept::{self, Allowance, Bot, Kept, Pause};
use crate::identity::{Asked, Author, CredentialCommand};
use crate::instance::{Forge, Source};
use crate::retry::{Failed, Judged, Kind, Limited, Policy};

/// GitHub's API.
pub const API: &str = "https://api.github.com";

/// Where the API calls go instead of [`API`]: a fake one, in tests.
pub const API_VAR: &str = "SLUICE_FORGE_API";

/// Where git reaches GitHub's repositories.
pub const GIT: &str = "https://github.com";

/// How long a JWT is good for, the most GitHub takes.
const JWT_LIFE: i64 = 600;
/// How far back a JWT is dated, for a host clock a little ahead.
const JWT_SKEW: i64 = 60;
/// How long before its end a token is bought again.
const REFRESH: TimeDelta = TimeDelta::minutes(5);
/// How old a token bought for a refused call is before it is handed over:
/// GitHub may refuse a token for its first seconds, until all its servers
/// know it (Octokit's auth-app waits as long for the same reason).
const SETTLE: TimeDelta = TimeDelta::seconds(5);
/// How long a pause lasts when GitHub names no time: doubled for each
/// such answer in a row, up to [`LONGEST_PAUSE`], as GitHub asks.
const FIRST_PAUSE: TimeDelta = TimeDelta::minutes(1);
const LONGEST_PAUSE: TimeDelta = TimeDelta::minutes(30);
/// How long the app's bot, once read, names the commits sluice pushes as
/// the app before it is read again (record 0019 as amended 2026-11-18).
const BOT_FOR: TimeDelta = TimeDelta::days(1);

/// The time now; a test's own clock in tests.
pub type Now = Box<dyn Fn() -> DateTime<Utc>>;

/// A repository on github.com.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    /// The repository a remote's URL names on github.com, in its HTTPS,
    /// `ssh://` or `git@github.com:` form; `None` for any other host.
    pub fn from_url(url: &str) -> Option<Repo> {
        let path = [
            "https://github.com/",
            "ssh://git@github.com/",
            "git@github.com:",
        ]
        .iter()
        .find_map(|p| url.strip_prefix(p))?;
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some(Repo {
            owner: owner.into(),
            name: name.into(),
        })
    }

    pub fn https(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.name)
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// The app's installation on a repository.
pub struct Installation {
    pub id: u64,
    /// Each permission the installation was granted: `contents` to
    /// `write`, and so on.
    pub permissions: BTreeMap<String, String>,
}

#[derive(Clone)]
struct Token {
    secret: String,
    /// Its end; none where a credential command gave none, when it is held
    /// until GitHub refuses it.
    ends: Option<DateTime<Utc>>,
    /// When sluice got it, bought or given.
    bought: DateTime<Utc>,
    /// The installation it was bought from; none for a command's token,
    /// which tells none.
    installation: Option<u64>,
    /// What it is, but for its age and end, built when it is got: whose,
    /// for what, from where and what GitHub says it reaches; never the
    /// secret.
    about: String,
}

/// The tokens a process holds, one for each app and repository, shared by
/// every [`App`] given it, on any thread (record 0108 as amended
/// 2026-11-19). Each repository's token is got under that repository's
/// own lock, so two calls that need it at once get it once, and a
/// repository's token never waits on another's. A clone holds the same
/// tokens.
#[derive(Clone, Default)]
pub struct Tokens(Arc<Mutex<BTreeMap<(u64, Repo), Slot>>>);

/// One app's token for one repository, while it holds one.
type Slot = Arc<Mutex<Option<Token>>>;

impl Tokens {
    /// App `app`'s place for its token for `repo`.
    fn slot(&self, app: u64, repo: &Repo) -> Slot {
        locked(&self.0)
            .entry((app, repo.clone()))
            .or_default()
            .clone()
    }
}

/// `m` locked, whatever a thread that panicked holding it left: a token
/// is whole or absent, never half written.
fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// How the app comes by a token for a repository: minted with its key, or
/// given by the host's credential command (record 0108).
enum Mint {
    Key(EncodingKey),
    Command(CredentialCommand),
}

/// Record 0119 (design D4): a token that reads one repository's contents
/// and nothing else, for one run build, and when GitHub ends it.
pub struct Reading {
    pub secret: String,
    pub ends: DateTime<Utc>,
}

/// Its end, never its secret.
impl std::fmt::Debug for Reading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a token that reads one repository, ending {}", self.ends)
    }
}

/// An installation token, and what it is for a report of its refusal.
pub struct Issued {
    pub secret: String,
    /// The app, the repository, the installation it was bought from, its
    /// scope and its age; for a command's, the command, what GitHub says
    /// it reaches, its age and its end; never the secret.
    pub about: String,
}

#[derive(Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

/// What a call to the API does, which decides how it is tried again (134;
/// rule core.29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// Changes nothing: a `GET`, a GraphQL query, a token bought.
    Read,
    /// Sets a state, which set twice is as set once: tried again as a
    /// read is.
    Set,
    /// Makes or starts something: tried again only while GitHub never
    /// took it.
    Make,
}

impl Call {
    /// The policy's kind of call this is.
    fn kind(self) -> Kind {
        match self {
            Call::Read | Call::Set => Kind::Read,
            Call::Make => Kind::Write,
        }
    }
}

/// What an API call authenticates as.
#[derive(Clone, Copy)]
pub enum Bearer<'a> {
    /// The app itself, with a JWT.
    App,
    /// The app's installation on a repository, with its token: one bought
    /// afresh for the next try when GitHub refuses it.
    Repo(&'a Repo),
    /// A token given, the operator's own.
    Token(&'a str),
    /// A token the host's credential command gave for a repository, asked
    /// with before it is held: counted against the repository's
    /// allowance, and a refusal of it no reason to get another (record
    /// 0108).
    Given(&'a Repo, &'a str),
}

pub struct App {
    id: u64,
    mint: Mint,
    api: String,
    /// Where git reaches the repositories: [`GIT`], or a test's fake.
    git: String,
    now: Now,
    /// The client its calls go through, made at its first call
    /// ([`super::client`]).
    client: std::cell::OnceCell<Client>,
    /// Where its connections go in place of the address a call names: a
    /// test's; none, where the call names.
    reroute: Option<Reroute>,
    /// The tokens it holds, its own or shared with the process's other
    /// apps ([`App::holding`]).
    tokens: Tokens,
    /// The app's bot, once read.
    login: RefCell<Option<String>>,
    /// The bot commits name, where nothing is kept on the host.
    bot: RefCell<Option<Bot>>,
    policy: Rc<Policy>,
    /// The host's answers and pauses; none, nothing is kept.
    kept: Option<Kept>,
    /// The end of the last pause this process said it waits out.
    told: Cell<Option<DateTime<Utc>>>,
}

/// What GitHub said of its allowance with an answer, and of the answer's
/// version.
#[derive(Default)]
struct Heard {
    remaining: Option<u64>,
    reset: Option<DateTime<Utc>>,
    resource: Option<String>,
    retry_after: Option<u64>,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// One call's answer: its status, its JSON, its text and what GitHub said
/// with it.
struct Reply {
    status: u16,
    v: Value,
    text: String,
    heard: Heard,
}

impl App {
    /// The app `id`, signing with the PEM private key `pem`, calling the
    /// API at `api`, each call under `policy`.
    pub fn new(id: u64, pem: &[u8], api: &str, now: Now, policy: Rc<Policy>) -> Result<App> {
        let key = EncodingKey::from_rsa_pem(pem).context("the app's key is no RSA private key")?;
        Ok(App::made(id, Mint::Key(key), api, now, policy))
    }

    /// The app `id`, its tokens given by `command` (record 0108), calling
    /// the API at `api`, each call under `policy`. Nothing is asked of the
    /// command before a token is needed.
    pub fn commanded(
        id: u64,
        command: CredentialCommand,
        api: &str,
        now: Now,
        policy: Rc<Policy>,
    ) -> App {
        App::made(id, Mint::Command(command), api, now, policy)
    }

    fn made(id: u64, mint: Mint, api: &str, now: Now, policy: Rc<Policy>) -> App {
        App {
            id,
            mint,
            api: api.trim_end_matches('/').into(),
            git: GIT.into(),
            now,
            client: std::cell::OnceCell::new(),
            reroute: None,
            tokens: Tokens::default(),
            login: RefCell::default(),
            bot: RefCell::default(),
            policy,
            kept: None,
            told: Cell::new(None),
        }
    }

    /// The command that gives the app's tokens, where the host names one in
    /// place of its key.
    pub fn command(&self) -> Option<&CredentialCommand> {
        match &self.mint {
            Mint::Command(c) => Some(c),
            Mint::Key(_) => None,
        }
    }

    /// The host's `[forge]` app, each call under the command's `policy`,
    /// keeping its answers and pauses in the host's state: its tokens
    /// minted with its key, or given by its credential command, which is
    /// run only once a token is needed (record 0108).
    pub fn open(forge: &Forge, policy: Rc<Policy>) -> Result<App> {
        let faked = std::env::var(API_VAR).ok();
        let api = faked.clone().unwrap_or_else(|| API.into());
        let app = match &forge.source {
            Source::Key(key) => {
                let pem = std::fs::read(key)
                    .with_context(|| format!("reading the app's key {}", key.display()))?;
                App::new(forge.app_id, &pem, &api, Box::new(Utc::now), policy)
                    .with_context(|| format!("[forge] key {}", key.display()))?
            }
            Source::Credential(c) => App::commanded(
                forge.app_id,
                CredentialCommand::new(&c.program, c.args.clone()),
                &api,
                Box::new(Utc::now),
                policy,
            ),
        };
        // Only a test's fake is named there, and it answers git's address
        // too.
        let app = match faked {
            Some(base) => app.serving_git(&base),
            None => app,
        };
        Ok(app.keeping(Kept::new(&crate::instance::state_dir()?)))
    }

    /// `self`, reaching the repositories' git at `base` instead of
    /// [`GIT`]: a test's fake.
    pub fn serving_git(mut self, base: &str) -> App {
        self.git = base.trim_end_matches('/').into();
        self
    }

    /// `self`, holding its tokens in `tokens`, which other apps of this
    /// process may hold theirs in too: a token any of them got for a
    /// repository is the one each takes for it, until it is got again
    /// (record 0108 as amended 2026-11-19).
    pub fn holding(mut self, tokens: Tokens) -> App {
        self.tokens = tokens;
        self
    }

    /// `self`, keeping its answers and pauses in `kept`.
    pub fn keeping(mut self, kept: Kept) -> App {
        self.kept = Some(kept);
        self
    }

    /// `self`, each connection given to `reroute` first: a test's, which
    /// can send a call where no connection is ever made.
    pub fn rerouting(mut self, reroute: Reroute) -> App {
        self.reroute = Some(reroute);
        self.client = std::cell::OnceCell::new();
        self
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// A JWT that authenticates as the app for ten minutes; none under a
    /// credential command, which signs nothing.
    pub fn jwt(&self) -> Result<String> {
        let Mint::Key(key) = &self.mint else {
            bail!("[forge] names a credential command, not a key");
        };
        let now = (self.now)().timestamp();
        let claims = Claims {
            iat: now - JWT_SKEW,
            exp: now - JWT_SKEW + JWT_LIFE,
            iss: self.id.to_string(),
        };
        Ok(jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &claims,
            key,
        )?)
    }

    /// The app's slug, which GitHub names its bot user after, read with
    /// its JWT.
    pub fn slug(&self) -> Result<String> {
        let v = self
            .get("/app", Bearer::App)
            .with_context(|| format!("reading app {}", self.id))?;
        Ok(v["slug"]
            .as_str()
            .ok_or_else(|| anyhow!("GitHub gave app {} no slug", self.id))?
            .to_string())
    }

    /// The app's bot, which GitHub names as the author of every pull
    /// request the app opens: under a key, named after its slug, read
    /// with its JWT; under a credential command, GraphQL's `viewer` read
    /// with `repo`'s token, since GitHub answers the app's own record to
    /// its key alone (C.5 pull request, 120; record 0108). Read once in
    /// this process, and kept.
    pub fn bot(&self, repo: &Repo) -> Result<String> {
        if let Some(login) = self.login.borrow().as_ref() {
            return Ok(login.clone());
        }
        let login = match self.mint {
            Mint::Key(_) => format!("{}[bot]", self.slug()?),
            Mint::Command(_) => self.viewer(repo)?,
        };
        *self.login.borrow_mut() = Some(login.clone());
        Ok(login)
    }

    /// The author and committer of a commit sluice pushes to `repo` as the
    /// app: its bot, the login at `<user id>+<login>@users.noreply.github.com`,
    /// the address GitHub links to the bot's account (record 0019 as
    /// amended 2026-11-18; rule core.43). A bot read less than a day ago by
    /// the app's clock, kept in the host's state for every process, or in
    /// this process where the app keeps nothing on the host, is used as it
    /// is; otherwise it is read afresh ([`App::read_bot`]) and kept. Where
    /// that read fails, a bot kept, however old, is used and nothing is
    /// written over it, so the next commit reads again; where none is kept,
    /// the failure is the commit's, in GitHub's words, and nothing is kept.
    pub fn author(&self, repo: &Repo) -> Result<Author> {
        let now = (self.now)();
        let kept = match &self.kept {
            Some(k) => k.bot(self.id),
            None => self.bot.borrow().clone(),
        };
        if let Some(bot) = kept.as_ref().filter(|b| now - b.read_at < BOT_FOR) {
            return Ok(authored(bot));
        }
        let bot = match (self.read_bot(repo), kept) {
            (Ok(bot), _) => bot,
            (Err(e), Some(bot)) => {
                tracing::warn!(
                    "app {}'s bot {} read at {} names the commit, as GitHub could not be read \
                     for it again: {e:#}",
                    self.id,
                    bot.login,
                    bot.read_at
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                );
                return Ok(authored(&bot));
            }
            (Err(e), None) => return Err(e),
        };
        match &self.kept {
            Some(k) => {
                if let Err(e) = k.keep_bot(self.id, &bot) {
                    tracing::warn!("keeping app {}'s bot {}: {e:#}", self.id, bot.login);
                }
            }
            None => *self.bot.borrow_mut() = Some(bot.clone()),
        }
        Ok(authored(&bot))
    }

    /// The app's bot read from GitHub now with its own credential: its
    /// login afresh, under a key its slug read with its JWT, then `[bot]`,
    /// and under a credential command GraphQL's `viewer` read with `repo`'s
    /// token; then its user id, `GET /users/<login>` with `repo`'s token. A
    /// login that does not end in `[bot]` is no app's bot, and fails naming
    /// it before any user is read (record 0019 as amended 2026-11-18).
    fn read_bot(&self, repo: &Repo) -> Result<Bot> {
        let login = match self.mint {
            Mint::Key(_) => format!("{}[bot]", self.slug()?),
            Mint::Command(_) => self.viewer(repo)?,
        };
        if !login.ends_with("[bot]") {
            bail!("GitHub names {repo}'s token {login}, who is no app's bot");
        }
        let path = format!("/users/{}", login.replace('[', "%5B").replace(']', "%5D"));
        let v = self
            .get(&path, Bearer::Repo(repo))
            .with_context(|| format!("reading app {}'s bot {login}", self.id))?;
        let login = v["login"]
            .as_str()
            .ok_or_else(|| anyhow!("GitHub gave the user {login} no login"))?
            .to_string();
        let id = v["id"]
            .as_u64()
            .ok_or_else(|| anyhow!("GitHub gave the user {login} no id"))?;
        Ok(Bot {
            login,
            id,
            read_at: (self.now)(),
        })
    }

    /// The app whose token `repo`'s is, by GitHub's word: its id and its
    /// slug, the viewer's login with its `[bot]` taken off, the app's
    /// record read by that slug with the token. What tells a command's
    /// token of another app, which would pass a ruleset's check against
    /// `[forge] app_id` and fail every push (record 0108).
    pub fn app_of_token(&self, repo: &Repo) -> Result<(u64, String)> {
        let login = self.viewer(repo)?;
        let Some(slug) = login.strip_suffix("[bot]") else {
            bail!("GitHub names the token {login}, who is no app's bot");
        };
        let v = self
            .get(&format!("/apps/{slug}"), Bearer::Repo(repo))
            .with_context(|| format!("reading app {slug}"))?;
        let id = v["id"]
            .as_u64()
            .ok_or_else(|| anyhow!("GitHub gave app {slug} no id"))?;
        Ok((id, slug.to_string()))
    }

    /// GitHub's status for git's first ask of a push to `repo` with its
    /// token, the receive-pack advertisement: 200 where the token may
    /// push, and 401, 403 or 404 where it may not. Read under the retry
    /// policy as a read (record 0108).
    pub fn may_push(&self, repo: &Repo) -> Result<u16> {
        use base64::Engine;
        let token = self.token(repo)?;
        let path = format!(
            "/{}/{}.git/info/refs?service=git-receive-pack",
            repo.owner, repo.name
        );
        let url = format!("{}{path}", self.git);
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{token}", super::TOKEN_USER));
        let auth = format!("Basic {basic}");
        self.policy
            .run(Kind::Read, |_| {
                let unread = |f: Failed<anyhow::Error>| {
                    Failed::new(f.judged, f.error.context(format!("GET {path}")))
                };
                let req = http::Request::get(&url)
                    .header("Authorization", &auth)
                    .header("Git-Protocol", "version=2")
                    .header("User-Agent", "sluice")
                    .body(String::new())
                    .map_err(|e| unread(Failed::new(Judged::Never, e.into())))?;
                let raw = self.client().map_err(unread)?.send(req).map_err(unread)?;
                match raw.status {
                    s @ 500..=599 => Err(Failed::new(
                        Judged::Never,
                        Unanswered(format!("GET {path}: {s}")).into(),
                    )),
                    s => Ok(s),
                }
            })
            .map_err(|s| s.into_error())
    }

    /// Who GitHub says `repo`'s token is, GraphQL's `viewer`: the bot of
    /// the app whose token it is.
    fn viewer(&self, repo: &Repo) -> Result<String> {
        let query = serde_json::json!({ "query": "query { viewer { login } }" }).to_string();
        let v = self
            .post_api(Call::Read, "/graphql", Bearer::Repo(repo), &query)
            .with_context(|| format!("asking GitHub whose {repo}'s token is"))?;
        Ok(v["data"]["viewer"]["login"]
            .as_str()
            .ok_or_else(|| anyhow!("GitHub named no viewer for {repo}'s token: {}", v["errors"]))?
            .to_string())
    }

    /// The app's installation on `repo`. GitHub finds none on a repository
    /// renamed or transferred since its remote was pointed at it, as on one
    /// the app was never installed on, and its answer says what to do.
    pub fn installation(&self, repo: &Repo) -> Result<Installation> {
        let v = self
            .get(
                &format!("/repos/{}/{}/installation", repo.owner, repo.name),
                Bearer::App,
            )
            .map_err(|e| match e.downcast::<Refused>() {
                Ok(r) if r.status == 404 => Refused {
                    note: Some(format!(
                        "GitHub finds no installation of app {} on {repo}: if the repository was \
                         renamed or transferred, point the remote that names it at its new \
                         address, which a running sluice reads at its next call; otherwise \
                         install the app on {repo}",
                        self.id
                    )),
                    ..r
                }
                .into(),
                Ok(r) => r.into(),
                Err(e) => e,
            })
            .with_context(|| format!("finding app {}'s installation on {repo}", self.id))?;
        let id = v["id"]
            .as_u64()
            .ok_or_else(|| anyhow!("the installation on {repo} has no id"))?;
        let permissions = v["permissions"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Installation { id, permissions })
    }

    /// An installation token for `repo`, bought, or got from the host's
    /// credential command, when this process has none that lasts five
    /// minutes more.
    pub fn token(&self, repo: &Repo) -> Result<String> {
        Ok(self.held(repo)?.secret)
    }

    /// A token for `repo` as [`App::token`] gives it, or, where GitHub
    /// `refused` a token, one got in its place: the token held where it is
    /// another, got since by another call, and otherwise one bought now,
    /// which replaces it. Under a credential command the refused token is
    /// erased and one got afresh, and a command that gives the refused
    /// token again gives none afresh (record 0108). A fresh token is handed
    /// over once it is [`SETTLE`] old.
    pub fn issue(&self, repo: &Repo, refused: Option<&str>) -> Result<Issued> {
        let t = match refused {
            Some(refused) => {
                let t = self.renewed(repo, refused)?;
                if let Ok(wait) = (SETTLE - ((self.now)() - t.bought)).to_std() {
                    std::thread::sleep(wait);
                }
                t
            }
            None => self.held(repo)?,
        };
        let age = ((self.now)() - t.bought).num_seconds().max(0);
        let about = match (t.installation, t.ends) {
            (Some(_), _) => format!("{}, bought {age}s ago", t.about),
            (None, Some(ends)) => format!(
                "{}, got {age}s ago, ending {}",
                t.about,
                ends.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ),
            (None, None) => format!("{}, got {age}s ago, given with no end", t.about),
        };
        Ok(Issued {
            about,
            secret: t.secret,
        })
    }

    /// The token held for `repo` while it lasts five minutes more, or one
    /// given with no end, or one got now under the repository's lock, so
    /// a call that waited on that lock takes the token got there.
    fn held(&self, repo: &Repo) -> Result<Token> {
        let slot = self.tokens.slot(self.id, repo);
        let mut held = locked(&slot);
        let now = (self.now)();
        if let Some(t) = held.as_ref() {
            match t.ends {
                Some(ends) if ends - now > REFRESH => return Ok(t.clone()),
                // Held until GitHub refuses it, as git holds a credential
                // with no expiry (record 0108).
                None => return Ok(t.clone()),
                Some(_) => {}
            }
        }
        let t = self.get_token(repo)?;
        *held = Some(t.clone());
        Ok(t)
    }

    /// A token got now for `repo` alone, bought with the app's key or
    /// given by the host's command.
    fn get_token(&self, repo: &Repo) -> Result<Token> {
        Ok(match &self.mint {
            Mint::Key(_) => self.buy(repo)?,
            Mint::Command(command) => {
                let given = command.get(&asked(repo))?;
                self.described(command, repo, given.password, given.ends)?
            }
        })
    }

    /// A token for `repo` in place of `refused`, which GitHub refused: the
    /// one held where another call got it since, and otherwise one got
    /// afresh in place of the one held; under a command, the refused one
    /// erased first, and the same one given again no fresh token (134;
    /// rule core.29; record 0108). It is got under the repository's lock,
    /// so two calls refused the same token get one afresh between them.
    fn renewed(&self, repo: &Repo, refused: &str) -> Result<Token> {
        let slot = self.tokens.slot(self.id, repo);
        let mut held = locked(&slot);
        if let Some(t) = held.as_ref().filter(|t| t.secret != refused) {
            return Ok(t.clone());
        }
        *held = None;
        let t = match &self.mint {
            Mint::Key(_) => self.buy(repo)?,
            Mint::Command(command) => {
                let asked = asked(repo);
                command.erase(&asked, super::TOKEN_USER, refused);
                let given = command.get(&asked)?;
                if given.password == refused {
                    bail!(
                        "[forge] credential {command} gave again the token GitHub refused for {repo}"
                    );
                }
                self.described(command, repo, given.password, given.ends)?
            }
        };
        *held = Some(t.clone());
        Ok(t)
    }

    /// The token `command` gave for `repo`, what it is read once from
    /// GitHub with it ([`App::reach`]); refused where GitHub says it does
    /// not reach `repo`.
    fn described(
        &self,
        command: &CredentialCommand,
        repo: &Repo,
        secret: String,
        ends: Option<DateTime<Utc>>,
    ) -> Result<Token> {
        let bought = (self.now)();
        let reach = self.reach(command, repo, &secret)?;
        Ok(Token {
            about: format!(
                "app {}'s token for {repo} from [forge] credential {command}, {reach}",
                self.id
            ),
            secret,
            ends,
            bought,
            installation: None,
        })
    }

    /// What GitHub says a token `command` gave for `repo` reaches, asked
    /// once with it, in words that follow the command in what the token
    /// is: `reaching` the repositories GitHub lists, or every one of the
    /// account; refused where GitHub's whole list leaves `repo` out, as a
    /// bought token is. The read is counted against `repo`'s allowance,
    /// not made while its pause stands, and GitHub's refusal of it is
    /// told in its words, the token handed over all the same, since the
    /// call it was got for meets that refusal itself (134 as amended
    /// 2026-11-09; record 0108).
    fn reach(&self, command: &CredentialCommand, repo: &Repo, secret: &str) -> Result<String> {
        let path = "/installation/repositories?per_page=100";
        let bearer = Bearer::Given(repo, secret);
        let paused = self
            .allowance(bearer, path)
            .and_then(|a| self.kept.as_ref()?.pause(&a))
            .filter(|p| p.until > self.policy.wall());
        if let Some(p) = paused {
            return Ok(format!(
                "not asked what it reaches: GitHub's allowance is spent until {}",
                crate::retry::clock(p.until)
            ));
        }
        let (status, v, text, _) = match self.answer("GET", Call::Read, path, bearer, None) {
            Ok(a) => a,
            Err(e) => {
                return Ok(match crate::retry::limited(&e) {
                    Some(l) => format!(
                        "not asked what it reaches: GitHub's allowance is spent until {}",
                        crate::retry::clock(l.until)
                    ),
                    None => format!("GitHub did not say what it reaches: {e:#}"),
                })
            }
        };
        if !(200..300).contains(&status) {
            let why = v["message"].as_str().unwrap_or(text.trim());
            return Ok(format!(
                "GitHub refused to say what it reaches: {status} {why}"
            ));
        }
        let names: Vec<&str> = v["repositories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["full_name"].as_str())
            .collect();
        let full = repo.to_string();
        let total = v["total_count"].as_u64().unwrap_or(names.len() as u64);
        let listed = names.iter().any(|n| n.eq_ignore_ascii_case(&full));
        let all = v["repository_selection"].as_str() == Some("all");
        let whole = total <= names.len() as u64;
        let named = || {
            if names.is_empty() {
                "no repository".to_string()
            } else {
                names.join(", ")
            }
        };
        if !listed && whole && !all {
            bail!(
                "[forge] credential {command} gave a token for {repo} that GitHub says reaches {}, \
                 not {repo}",
                named()
            );
        }
        Ok(if all {
            format!("reaching every repository of {}", repo.owner)
        } else if listed {
            format!("reaching {}", named())
        } else {
            format!(
                "reaching {total} repositories, of which GitHub listed {}",
                names.len()
            )
        })
    }

    /// A token bought now from `repo`'s installation for `repo` alone.
    fn buy(&self, repo: &Repo) -> Result<Token> {
        let installation = self.installation(repo)?;
        let body = serde_json::json!({ "repositories": [repo.name] }).to_string();
        // Buying a token changes nothing the repository holds: a read.
        let v = self
            .send(
                "POST",
                Call::Read,
                &format!("/app/installations/{}/access_tokens", installation.id),
                Bearer::App,
                Some(&body),
            )
            .with_context(|| format!("making a token for {repo}"))?;
        let bought = (self.now)();
        let secret = v["token"]
            .as_str()
            .ok_or_else(|| anyhow!("GitHub gave no token for {repo}"))?
            .to_string();
        let ends = v["expires_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .ok_or_else(|| anyhow!("GitHub gave a token for {repo} with no end"))?
            .with_timezone(&Utc);
        Ok(Token {
            about: format!(
                "app {}'s token for {repo} from installation {}, scoped to {}",
                self.id,
                installation.id,
                scope(repo, &v)?
            ),
            secret,
            ends: Some(ends),
            bought,
            installation: Some(installation.id),
        })
    }

    /// Record 0119 (design D4 of the change
    /// `a-codebuild-fleet-runs-a-queues-checks`): a token of the app's
    /// installation on `repo` that reads `repo`'s contents and nothing
    /// else, `contents: read` on `repo` alone, bought now for one run build
    /// and never held by this process, ending when GitHub's answer says,
    /// within its hour. A token GitHub gave more than that is refused, and
    /// a host whose `[forge]` names a credential command in place of the
    /// app's key buys none, since it cannot narrow what it is given.
    pub fn reading(&self, repo: &Repo) -> Result<Reading> {
        if let Mint::Command(command) = &self.mint {
            bail!(
                "a run build reads {repo} with a token of contents: read on it alone, which \
                 sluice buys with the app's key; [forge] credential {command} gives tokens \
                 sluice cannot narrow, so name the app's key as [forge] key on a host that runs \
                 the queue of a branch on a CodeBuild pool"
            );
        }
        let installation = self.installation(repo)?;
        let body = serde_json::json!({
            "repositories": [repo.name],
            "permissions": { "contents": "read" },
        })
        .to_string();
        // Buying a token changes nothing the repository holds: a read.
        let v = self
            .send(
                "POST",
                Call::Read,
                &format!("/app/installations/{}/access_tokens", installation.id),
                Bearer::App,
                Some(&body),
            )
            .with_context(|| format!("making a token that reads {repo}"))?;
        let secret = v["token"]
            .as_str()
            .ok_or_else(|| anyhow!("GitHub gave no token that reads {repo}"))?
            .to_string();
        let ends = v["expires_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .ok_or_else(|| anyhow!("GitHub gave a token that reads {repo} with no end"))?
            .with_timezone(&Utc);
        if let Some(more) = v["permissions"].as_object().and_then(|p| {
            p.iter().find(|(k, v)| {
                v.as_str() != Some("read") || !["contents", "metadata"].contains(&k.as_str())
            })
        }) {
            bail!(
                "GitHub gave a token for reading {repo} that also holds {}: {}",
                more.0,
                more.1
            );
        }
        let names: Vec<&str> = v["repositories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["full_name"].as_str())
            .collect();
        if names.len() != 1 || !names[0].eq_ignore_ascii_case(&repo.to_string()) {
            bail!(
                "GitHub gave a token for reading {repo} that reaches {}",
                if names.is_empty() {
                    "no repository it named".to_string()
                } else {
                    names.join(", ")
                }
            );
        }
        Ok(Reading { secret, ends })
    }

    /// `GET` an API path as `bearer`.
    pub fn get(&self, path: &str, bearer: Bearer) -> Result<Value> {
        self.send("GET", Call::Read, path, bearer, None)
    }

    /// `POST` to an API path as `bearer` a call that does what `call`
    /// says.
    pub fn post_api(&self, call: Call, path: &str, bearer: Bearer, body: &str) -> Result<Value> {
        self.send("POST", call, path, bearer, Some(body))
    }

    /// `PATCH` an API path as `bearer`: every `PATCH` sluice makes sets a
    /// state.
    pub(super) fn patch_api(&self, path: &str, bearer: Bearer, body: &str) -> Result<Value> {
        self.send("PATCH", Call::Set, path, bearer, Some(body))
    }

    fn send(
        &self,
        method: &str,
        call: Call,
        path: &str,
        bearer: Bearer,
        body: Option<&str>,
    ) -> Result<Value> {
        let (status, v, text, _) = self.answer(method, call, path, bearer, body)?;
        if !(200..300).contains(&status) {
            return Err(failed(method, path, status, &v, &text));
        }
        Ok(v)
    }

    /// `GET` an API path as `bearer`: its JSON, and whether GitHub sent it
    /// afresh, `false` when it answered that the answer the host keeps for
    /// the path stands (a 304; 136).
    pub(super) fn get_fresh(&self, path: &str, bearer: Bearer) -> Result<(Value, bool)> {
        let (status, v, text, fresh) = self.answer("GET", Call::Read, path, bearer, None)?;
        if !(200..300).contains(&status) {
            return Err(failed("GET", path, status, &v, &text));
        }
        Ok((v, fresh))
    }

    /// `GET` an API path; `None` when GitHub has nothing there (404).
    pub(super) fn get_found(&self, path: &str, bearer: Bearer) -> Result<Option<Value>> {
        let (status, v, text, _) = self.answer("GET", Call::Read, path, bearer, None)?;
        match status {
            404 => Ok(None),
            200..=299 => Ok(Some(v)),
            _ => Err(failed("GET", path, status, &v, &text)),
        }
    }

    /// GitHub's answer to a `call`, under the retry policy: its status,
    /// its JSON, its text and whether GitHub sent it afresh rather than
    /// answering a `GET` from what the host keeps, for a success or any
    /// status GitHub would answer again. A 5xx is tried again, but on a call that makes
    /// something, which GitHub may have taken; a 401 on the installation's
    /// token is tried again on one bought afresh; a spent allowance is
    /// the policy's to wait out or stop on, no call made while its pause
    /// stands. The last try's failure is the call's when the policy stops;
    /// a call that makes something stopped with its answer not read fails
    /// [`Unanswered`], saying so.
    fn answer(
        &self,
        method: &str,
        call: Call,
        path: &str,
        bearer: Bearer,
        body: Option<&str>,
    ) -> Result<(u16, Value, String, bool)> {
        // The last failure's words, for a token that could not be bought
        // afresh after it.
        let mut said = String::new();
        // The installation's token the last try sent, which a renewal
        // replaces only where it is still the one held.
        let mut tried: Option<String> = None;
        self.policy
            .run(call.kind(), |t| {
                // A pause is known before any token is got, so no token is
                // got for a call it stops (record 0108).
                let allowance = self.allowance(bearer, path);
                let paused = allowance
                    .as_ref()
                    .and_then(|a| self.kept.as_ref()?.pause(a));
                if let Some(p) = paused.as_ref().filter(|p| p.until > self.policy.wall()) {
                    return Err(self.limited(p.until, &p.words));
                }
                let auth = match bearer {
                    Bearer::App => self.jwt(),
                    Bearer::Repo(repo) if t.renew() => {
                        let refused = tried.take();
                        self.issue(repo, refused.as_deref())
                            .map(|i| i.secret)
                            .map_err(|e| {
                                let how = match self.mint {
                                    Mint::Key(_) => "bought",
                                    Mint::Command(_) => "got",
                                };
                                anyhow!("{said}; and no token could be {how} afresh: {e:#}")
                            })
                    }
                    Bearer::Repo(repo) => self.token(repo),
                    Bearer::Token(token) | Bearer::Given(_, token) => Ok(token.to_string()),
                };
                let auth = auth.map_err(|e| match call {
                    // Nothing was sent: no answer to it went unread.
                    Call::Make => Failed::new(Judged::Answered, anyhow!("{e:#}")),
                    _ => Failed::new(Judged::Answered, e),
                })?;
                if let Bearer::Repo(_) = bearer {
                    tried = Some(auth.clone());
                }
                let url = format!("{}{path}", self.api);
                let kept = match (&self.kept, method) {
                    (Some(k), "GET") => k.answer(&url),
                    _ => None,
                };
                let r = self.call(method, path, &auth, body, kept.as_ref())?;
                if let Some(words) = spent(path, &r) {
                    return Err(self.pause(allowance.as_ref(), paused, &r.heard, words));
                }
                let judged = match r.status {
                    500..=599 if call == Call::Make => Judged::Unknown,
                    500..=599 => Judged::Never,
                    401 if matches!(bearer, Bearer::Repo(_)) => Judged::Credential,
                    _ => {
                        self.heard(allowance.as_ref(), paused, &r.heard);
                        return Ok(self.kept_answer(&url, method, kept, r));
                    }
                };
                let e = failed(method, path, r.status, &r.v, &r.text);
                said = format!("{e:#}");
                Err(Failed::new(judged, e))
            })
            .map_err(|s| match (call, s.judged) {
                (Call::Make, Judged::Unknown) => {
                    let words = format!("{:#}", s.error);
                    let called = format!("{method} {path}: ");
                    let words = words.strip_prefix(&called).unwrap_or(&words);
                    Unanswered(format!(
                        "GitHub's answer to {method} {path} was not read: {words}"
                    ))
                    .into()
                }
                _ => s.into_error(),
            })
    }

    /// The allowance a call as `bearer` to `path` is counted against:
    /// the installation's, named by the repository's owner, `graphql` or
    /// `core`, known before any token is held; `None` for the app's own
    /// calls and the operator's token (134; record 0108).
    fn allowance(&self, bearer: Bearer, path: &str) -> Option<Allowance> {
        let (Bearer::Repo(repo) | Bearer::Given(repo, _)) = bearer else {
            return None;
        };
        Some(Allowance {
            app: self.id,
            owner: repo.owner.clone(),
            resource: resource(path).into(),
        })
    }

    /// A try not made, or refused, for an allowance spent until `until`;
    /// said once on stderr when the policy waits it out.
    fn limited(&self, until: DateTime<Utc>, words: &str) -> Failed<anyhow::Error> {
        // Said once by this binding, and once by a wait over every step
        // `sluice host` takes of it (the change
        // `sluice-host-makes-each-change`, design D2).
        if self.policy.waits() && self.told.get() != Some(until) && crate::caller::untold(until) {
            self.told.set(Some(until));
            // Said to the caller, whose call `sluice host` makes keeps
            // what it says on stderr (the change
            // `sluice-host-makes-each-change`, design D1).
            crate::telemetry::say(&format!(
                "sluice: GitHub's allowance of calls is spent until {}; waiting: {words}",
                crate::retry::clock(until)
            ));
        }
        let e = Limited {
            app: self.id,
            until,
            words: words.into(),
        };
        Failed::new(Judged::Limited { until }, e.into())
    }

    /// GitHub said in `words` that an allowance is spent: the pause, until
    /// `retry-after`, else the reset when no call remains, else a minute
    /// doubled for each such answer in a row; kept for `allowance`, the
    /// resource GitHub named winning, later than any kept already.
    fn pause(
        &self,
        allowance: Option<&Allowance>,
        paused: Option<Pause>,
        heard: &Heard,
        words: String,
    ) -> Failed<anyhow::Error> {
        let now = self.policy.wall();
        let streak = paused.map_or(0, |p| p.streak);
        let named = match (heard.retry_after, heard.remaining, heard.reset) {
            (Some(secs), _, _) => Some(now + TimeDelta::seconds(secs as i64)),
            (None, Some(0), Some(reset)) => Some(reset),
            _ => None,
        };
        let until = named.unwrap_or_else(|| {
            now + (FIRST_PAUSE * 2i32.saturating_pow(streak.min(5))).min(LONGEST_PAUSE)
        });
        let pause = Pause {
            until,
            words,
            streak: streak + 1,
        };
        let pause = match (&self.kept, allowance) {
            (Some(k), Some(a)) => {
                let a = Allowance {
                    resource: heard.resource.clone().unwrap_or_else(|| a.resource.clone()),
                    ..a.clone()
                };
                match k.put_pause(&a, pause.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("keeping GitHub's pause: {e:#}");
                        pause
                    }
                }
            }
            _ => pause,
        };
        self.limited(pause.until, &pause.words)
    }

    /// GitHub answered: an answer that leaves no call of the allowance
    /// starts its pause until the reset, and any other ends the run of
    /// spent answers a pause kept.
    fn heard(&self, allowance: Option<&Allowance>, paused: Option<Pause>, heard: &Heard) {
        let (Some(k), Some(a)) = (&self.kept, allowance) else {
            return;
        };
        let a = Allowance {
            resource: heard.resource.clone().unwrap_or_else(|| a.resource.clone()),
            ..a.clone()
        };
        let pause = match (heard.remaining, heard.reset) {
            (Some(0), Some(until)) => Pause {
                until,
                words: "x-ratelimit-remaining: 0".into(),
                streak: 0,
            },
            _ => match paused {
                Some(p) if p.streak > 0 => Pause { streak: 0, ..p },
                _ => return,
            },
        };
        if let Err(e) = k.put_pause(&a, pause) {
            tracing::warn!("keeping GitHub's pause: {e:#}");
        }
    }

    /// The answer to a call GitHub judged, and whether GitHub sent it
    /// afresh: a `GET`'s 304 answered from `kept`, which it did not, its
    /// 200 kept for `url` when it has a version, its 404 forgetting what
    /// was kept.
    fn kept_answer(
        &self,
        url: &str,
        method: &str,
        kept: Option<kept::Answer>,
        r: Reply,
    ) -> (u16, Value, String, bool) {
        let Some(k) = self.kept.as_ref().filter(|_| method == "GET") else {
            return (r.status, r.v, r.text, true);
        };
        match (r.status, kept) {
            (304, Some(a)) => {
                k.used(url);
                let v = serde_json::from_str(&a.body).unwrap_or(Value::Null);
                (200, v, a.body, false)
            }
            (200, _) if r.heard.etag.is_some() || r.heard.last_modified.is_some() => {
                let a = kept::Answer {
                    url: url.into(),
                    etag: r.heard.etag,
                    last_modified: r.heard.last_modified,
                    body: r.text.clone(),
                    at: (self.now)(),
                };
                if let Err(e) = k.keep(&a) {
                    tracing::warn!("keeping GitHub's answer to {url}: {e:#}");
                }
                (r.status, r.v, r.text, true)
            }
            (404, _) => {
                k.forget(url);
                (r.status, r.v, r.text, true)
            }
            _ => (r.status, r.v, r.text, true),
        }
    }

    /// The client its calls go through, made at its first call; one that
    /// could not be made sent nothing.
    fn client(&self) -> Result<&Client, Failed<anyhow::Error>> {
        if let Some(c) = self.client.get() {
            return Ok(c);
        }
        let c = Client::new(self.reroute.clone()).map_err(|e| Failed::new(Judged::Never, e))?;
        Ok(self.client.get_or_init(|| c))
    }

    /// One call, conditional on `kept` when it is given: its answer; a
    /// call that got no answer read, how it stands.
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<&str>,
        kept: Option<&kept::Answer>,
    ) -> Result<Reply, Failed<anyhow::Error>> {
        let url = format!("{}{path}", self.api);
        let unread = |judged: Judged, e: anyhow::Error| {
            Failed::new(judged, e.context(format!("{method} {path}")))
        };
        let mut req = http::Request::builder()
            .uri(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "sluice")
            .header("Authorization", format!("Bearer {bearer}"));
        let req = match (method, body) {
            ("GET", _) => {
                req = req.method(http::Method::GET);
                if let Some(etag) = kept.and_then(|k| k.etag.as_deref()) {
                    req = req.header("If-None-Match", etag);
                }
                if let Some(at) = kept.and_then(|k| k.last_modified.as_deref()) {
                    req = req.header("If-Modified-Since", at);
                }
                req.body(String::new())
            }
            ("POST" | "PATCH", body) => req
                .method(method)
                .header("Content-Type", "application/json")
                .body(body.unwrap_or("").to_string()),
            (other, _) => {
                let e = anyhow!("sluice makes no {other} call");
                return Err(Failed::new(Judged::Answered, e));
            }
        }
        .map_err(|e| unread(Judged::Never, e.into()))?;
        let raw = self
            .client()
            .and_then(|c| c.send(req))
            .map_err(|f| unread(f.judged, f.error))?;
        let header = |name: &str| {
            raw.headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let Budget {
            remaining,
            reset,
            resource,
            retry_after,
        } = raw.budget.clone();
        let heard = Heard {
            remaining,
            reset,
            resource,
            retry_after,
            etag: header("etag"),
            last_modified: header("last-modified"),
        };
        let (status, text) = (raw.status, raw.text);
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(Reply {
            status,
            v,
            text,
            heard,
        })
    }
}

/// The author `bot` names: its login, at the address GitHub links to its
/// account.
fn authored(bot: &Bot) -> Author {
    Author {
        name: bot.login.clone(),
        email: format!("{}+{}@users.noreply.github.com", bot.id, bot.login),
    }
}

/// What a credential command is asked for a token for `repo`, in git's
/// words (record 0108).
fn asked(repo: &Repo) -> Asked {
    Asked {
        protocol: "https".into(),
        host: "github.com".into(),
        path: repo.to_string(),
    }
}

/// GitHub's name for the allowance a call to `path` is counted against.
fn resource(path: &str) -> &'static str {
    if path == "/graphql" {
        "graphql"
    } else {
        "core"
    }
}

/// GitHub's words, when `r` says an allowance is spent: a 429; a 403 that
/// leaves no call, names a time to call again or says `rate limit`; a
/// GraphQL answer whose errors say `RATE_LIMITED`.
fn spent(path: &str, r: &Reply) -> Option<String> {
    let message = || {
        r.v["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| r.text.trim().to_string())
    };
    match r.status {
        429 => Some(message()),
        403 if r.heard.remaining == Some(0)
            || r.heard.retry_after.is_some()
            || message().to_lowercase().contains("rate limit") =>
        {
            Some(message())
        }
        200 if path == "/graphql" => r.v["errors"]
            .as_array()?
            .iter()
            .find(|e| e["type"] == "RATE_LIMITED")
            .map(|e| e["message"].as_str().unwrap_or("RATE_LIMITED").to_string()),
        _ => None,
    }
}

/// What a token GitHub gave for `repo` reaches, in GitHub's words: its
/// repositories, then each permission. A token whose repositories leave
/// out `repo` is refused, since it is used for `repo` alone.
fn scope(repo: &Repo, v: &Value) -> Result<String> {
    let names: Vec<&str> = v["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["full_name"].as_str())
        .collect();
    let full = repo.to_string();
    if v["repositories"].is_array() && !names.iter().any(|n| n.eq_ignore_ascii_case(&full)) {
        bail!(
            "GitHub gave a token for {repo} that reaches {}, not {repo}",
            if names.is_empty() {
                "no repository".to_string()
            } else {
                names.join(", ")
            }
        );
    }
    let reaches = if names.is_empty() {
        match v["repository_selection"].as_str() {
            Some(s) => format!("{s} repositories"),
            None => "repositories GitHub did not list".to_string(),
        }
    } else {
        names.join(", ")
    };
    let granted: Vec<String> = v["permissions"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some(format!("{k}: {}", v.as_str()?)))
        .collect();
    Ok(if granted.is_empty() {
        reaches
    } else {
        format!("{reaches} with {}", granted.join(", "))
    })
}

/// GitHub's answer to a call, neither a success nor a 5xx: its words, and
/// sluice's own on what to do where it knows.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    /// The call and GitHub's answer: `GET /repos/o/r/pulls: 404 Not Found`.
    pub said: String,
    /// What to do; never GitHub's words.
    pub note: Option<String>,
}

impl std::fmt::Display for Refused {
    /// GitHub's words, then the note on the next line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.said)?;
        if let Some(note) = &self.note {
            write!(f, "\n{note}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Refused {}

/// The [`Refused`] `e` is, or that caused it.
pub fn refused(e: &anyhow::Error) -> Option<&Refused> {
    e.chain().find_map(|c| c.downcast_ref::<Refused>())
}

/// A call GitHub answered with `status`, not a success, in its words: a
/// 5xx is [`Unanswered`], GitHub unable to answer just then; any other is
/// [`Refused`], a redirect from a repository's address saying to point the
/// remote that names it at its new one.
fn failed(method: &str, path: &str, status: u16, v: &Value, text: &str) -> anyhow::Error {
    let why = v["message"].as_str().unwrap_or(text.trim());
    let said = format!("{method} {path}: {status} {why}");
    if (500..600).contains(&status) {
        return Unanswered(said).into();
    }
    let note = match (status, repository(path)) {
        (301 | 302 | 307 | 308, Some(repo)) => Some(format!(
            "GitHub answers {repo} from another address, as it does once a repository is \
             renamed or transferred: point the remote that names {repo} at the repository's new \
             address, which a running sluice reads at its next call"
        )),
        _ => None,
    };
    Refused { status, said, note }.into()
}

/// The repository, `owner/name`, a call's `path` is to, when it is one
/// under `/repos/`.
fn repository(path: &str) -> Option<String> {
    let mut parts = path.strip_prefix("/repos/")?.split(['/', '?']);
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, Validation};

    const KEY: &[u8] = include_bytes!("../../../tests/profiles/forge/test-key.pem");
    const PUBLIC: &[u8] = include_bytes!("../../../tests/profiles/forge/test-key.pub.pem");

    /// A JWT signed with the app's key verifies with its public half and
    /// carries the app's id and ten minutes of life.
    #[test]
    fn a_jwt_verifies_with_the_public_half() {
        let at = Utc::now();
        let policy = Rc::new(Policy::command());
        let app = App::new(4242, KEY, API, Box::new(move || at), policy).unwrap();
        let jwt = app.jwt().unwrap();
        let mut v = Validation::new(Algorithm::RS256);
        v.set_required_spec_claims(&["exp", "iat", "iss"]);
        v.set_issuer(&["4242"]);
        let data = jsonwebtoken::decode::<serde_json::Value>(
            &jwt,
            &DecodingKey::from_rsa_pem(PUBLIC).unwrap(),
            &v,
        )
        .unwrap();
        let c = data.claims;
        assert_eq!(c["iss"], "4242");
        let (iat, exp) = (c["iat"].as_i64().unwrap(), c["exp"].as_i64().unwrap());
        assert_eq!(exp - iat, 600);
        assert!(iat <= at.timestamp() && exp > at.timestamp());
    }

    /// Requests a server on this host answers, each with what `answer`
    /// gives for its method, path and body, until the test ends; and each
    /// request's method, path and body as it came.
    fn serve(
        answer: impl Fn(&str, &str, &str) -> (u16, String) + Send + 'static,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
    ) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut words = line.split_whitespace();
                let (method, path) = (
                    words.next().unwrap_or_default().to_string(),
                    words.next().unwrap_or_default().to_string(),
                );
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = header.split_once(':') {
                        if k.eq_ignore_ascii_case("content-length") {
                            length = v.trim().parse().unwrap();
                        }
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                let (status, text) = answer(&method, &path, &body);
                kept.lock().unwrap().push((method, path, body));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (address, seen)
    }

    /// Record 0119 (design D4): the token a run build reads its repository
    /// with is asked of GitHub with `contents: read` on that repository
    /// alone, bought for each start and held by none; one GitHub gave more
    /// than that is refused.
    #[test]
    fn a_reading_token_asks_to_read_one_repository() {
        let ends = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let wider = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let widened = wider.clone();
        let (api, seen) = serve(move |method, path, _| match (method, path) {
            ("GET", "/repos/acme/widgets/installation") => (
                200,
                r#"{"id": 99, "permissions": {"contents": "write"}}"#.into(),
            ),
            ("POST", "/app/installations/99/access_tokens") => {
                let contents = match widened.load(std::sync::atomic::Ordering::SeqCst) {
                    true => "write",
                    false => "read",
                };
                (
                    201,
                    serde_json::json!({
                        "token": "ghs_reads",
                        "expires_at": ends,
                        "permissions": { "contents": contents, "metadata": "read" },
                        "repositories": [{ "full_name": "acme/widgets" }],
                    })
                    .to_string(),
                )
            }
            _ => (404, r#"{"message": "Not Found"}"#.into()),
        });
        let policy = Rc::new(Policy::command().recording());
        let app = App::new(4242, KEY, &api, Box::new(Utc::now), policy).unwrap();
        let repo = Repo {
            owner: "acme".into(),
            name: "widgets".into(),
        };
        let token = app.reading(&repo).unwrap();
        assert_eq!(token.secret, "ghs_reads");
        assert!(token.ends > Utc::now());
        app.reading(&repo).unwrap();
        let asked: Vec<(String, String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, path, _)| path.ends_with("/access_tokens"))
            .cloned()
            .collect();
        assert_eq!(asked.len(), 2, "bought for each start, held by none");
        let body: Value = serde_json::from_str(&asked[0].2).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "repositories": ["widgets"], "permissions": { "contents": "read" } })
        );
        wider.store(true, std::sync::atomic::Ordering::SeqCst);
        let e = format!("{:#}", app.reading(&repo).unwrap_err());
        assert!(e.contains("also holds contents"), "{e}");
    }

    /// Record 0119 (design D4; core.26): a host whose `[forge]` names a
    /// credential command buys no reading token, since it cannot narrow
    /// what the command gives, and says to name the app's key on a host
    /// that runs the queue of a branch on a CodeBuild pool; nothing is
    /// asked of the command or of GitHub.
    #[test]
    fn a_credential_command_buys_no_reading_token() {
        let command = CredentialCommand::new(
            "/opt/forge/bin/token-helper",
            vec!["--app".into(), "sluice".into()],
        );
        let policy = Rc::new(Policy::command());
        let app = App::commanded(4242, command, API, Box::new(Utc::now), policy);
        let repo = Repo {
            owner: "acme".into(),
            name: "widgets".into(),
        };
        assert_eq!(
            format!("{:#}", app.reading(&repo).unwrap_err()),
            "a run build reads acme/widgets with a token of contents: read on it alone, which \
             sluice buys with the app's key; [forge] credential /opt/forge/bin/token-helper \
             --app sluice gives tokens sluice cannot narrow, so name the app's key as [forge] \
             key on a host that runs the queue of a branch on a CodeBuild pool"
        );
    }

    /// A 5xx is GitHub unable to answer just then, told apart from any
    /// other failure; the words are GitHub's either way.
    #[test]
    fn a_5xx_is_unanswered_and_a_4xx_is_not() {
        let path = "/repos/o/r/pulls/7";
        let e = failed("GET", path, 503, &Value::Null, "Service Unavailable\n");
        assert!(sluice_seams::unanswered(&e));
        assert!(sluice_seams::unanswered(&e.context("following #7")));
        let e = failed("GET", path, 503, &Value::Null, "Service Unavailable\n");
        assert_eq!(
            e.to_string(),
            format!("GET {path}: 503 Service Unavailable")
        );
        let e = failed(
            "GET",
            path,
            404,
            &serde_json::json!({"message": "Not Found"}),
            "",
        );
        assert!(!sluice_seams::unanswered(&e));
        assert_eq!(e.to_string(), format!("GET {path}: 404 Not Found"));
    }

    #[test]
    fn a_remote_on_github_and_others() {
        let r = Repo {
            owner: "o".into(),
            name: "r".into(),
        };
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
        ] {
            assert_eq!(Repo::from_url(url), Some(r.clone()), "{url}");
        }
        assert_eq!(r.https(), "https://github.com/o/r.git");
        for url in [
            "https://gitlab.com/o/r.git",
            "/srv/git/r.git",
            "https://github.com/o",
            "https://github.com/o/r/x",
        ] {
            assert_eq!(Repo::from_url(url), None, "{url}");
        }
    }
}
