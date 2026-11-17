//! What the host keeps of its calls to GitHub, in `forge/` under the
//! host's state, for every process of sluice on the host (134, 136; rules
//! core.29, core.32; ADR 0071):
//!
//! - `answers/<h>.json`: the last answer at each address, its ETag and
//!   last-modified time with its body, `<h>` the first 32 hex digits of
//!   the address's SHA-256. It is used only on GitHub's 304 for it, so it
//!   decides nothing GitHub did not just confirm (rule core.11).
//! - `pauses/<app>-<owner>-<resource>.json`: until when GitHub said an
//!   allowance is spent, in its words, and how many such answers came in a
//!   row; `owner` is the account the repository is on, which names the
//!   app's one installation there, whichever way its token came, minted
//!   from the app's key or given by the host's credential command, which
//!   tells no installation's id (record 0108); `resource` is GitHub's own
//!   name for the allowance, `core` for REST and `graphql`.
//! - `requests/<owner>/<repo>/<base>.json`: each branch's readings, when
//!   its open pull requests were last asked for, at which revision of the
//!   branch and version of its records, and what was last read and posted
//!   of each request ([`Readings`]), changed under the exclusive lock of
//!   the `.lock` beside it, which goes with its process. It says only what
//!   was last read, so that a request that has not moved is not read
//!   again (rule core.14).
//! - `requests/<owner>/<repo>/<base>.moved`: touched by a submit on the
//!   host that opened, updated or marked ready a request into the branch;
//!   newer than the last asking, it asks at once.
//! - `follows/<owner>/<repo>/<base>.json`: on a host that does not keep
//!   the branch, each pull request it follows ([`Follows`]): one a submit
//!   on the host opened, updated or marked ready, or one a wait or status
//!   on it followed, by number, with when
//!   GitHub was last read for it, what that read found of it and sluice's
//!   check on its head; changed under the exclusive lock of the `.lock`
//!   beside it, so that every process of the host reads a request once in
//!   a quarter of the interval between them (137, design D3, D4).
//!
//! An `intake/` an older sluice kept here is read by nothing, and may be
//! removed (design D23).
//!
//! Each file is written whole to a file of its own and renamed over the
//! last, so a reader finds a whole one or the one before; a file that does
//! not parse is read as absent, and written again by the next answer.
//! Answers not used for a day are removed by a process that keeps one, at
//! most once an hour. Nothing here holds a token.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::app::Repo;
use super::forge::Posted;

/// How long an answer no call used is kept.
const UNUSED: Duration = Duration::from_secs(24 * 60 * 60);
/// How often a process that keeps an answer looks for unused ones.
const SWEEP: Duration = Duration::from_secs(60 * 60);
/// The file whose time says when answers were last looked over.
const SWEPT: &str = ".swept";

/// The last answer GitHub gave at an address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: String,
    pub at: DateTime<Utc>,
}

/// GitHub's word that an allowance is spent: until when, in its words,
/// and how many such answers came in a row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub until: DateTime<Utc>,
    pub words: String,
    pub streak: u32,
}

/// Which allowance a pause is of: the app's installation, named by the
/// account it is on, since an app has one installation on an account, and
/// GitHub's name for the allowance (134; record 0108).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowance {
    pub app: u64,
    pub owner: String,
    pub resource: String,
}

/// A branch's readings: when its open pull requests were last asked for,
/// at which revision of the branch and which version of its records, and
/// what was last read of each open request, by number (136).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readings {
    pub asked: Option<DateTime<Utc>>,
    pub tip: Option<String>,
    pub version: Option<u64>,
    #[serde(default)]
    pub requests: BTreeMap<u64, Reading>,
}

/// What was last read and posted of one pull request (128, 136).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    /// Its head, last change and draft mark as the list last said.
    pub head: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub draft: bool,
    /// Its latest submission on this host and that one's state, by word.
    pub entry: Option<(u64, String)>,
    /// What it lacked of its conditions when they were last read.
    pub missing: Option<Vec<String>>,
    /// When it was marked ready for review, as its timeline last said.
    pub readied: Option<Vec<DateTime<Utc>>>,
    /// The head sluice's check runs were last posted or read on, and each
    /// of them there by its name.
    pub posted_on: Option<String>,
    #[serde(default)]
    pub posted: BTreeMap<String, Posted>,
    /// The forecast it was last shown with, while the branch and its
    /// records stand.
    pub forecast: Option<String>,
    /// The id of sluice's report comment on it, and a digest of what the
    /// comment says.
    pub report: Option<u64>,
    pub report_said: Option<String>,
    /// The notifications a comment on it delivered.
    #[serde(default)]
    pub said: Vec<String>,
    /// Whether this pass's one query read it whole, so that what it lacks
    /// stands for the pass, whatever it is; never kept.
    #[serde(skip)]
    pub whole: bool,
}

/// 137: the pull requests into a branch a host that does not take them
/// in follows, by number (design D3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Follows {
    #[serde(default)]
    pub requests: BTreeMap<u64, Follow>,
}

/// 137: what a host last read of one pull request it follows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Follow {
    /// When a process of the host last read it and sluice's check on its
    /// head from GitHub, or began to; `None` where the host knows it only
    /// by its own act on it, so the next process to follow it reads it.
    pub read_at: Option<DateTime<Utc>>,
    pub head: String,
    pub open: bool,
    pub merged: bool,
    pub draft: bool,
    pub title: String,
    /// The branch it is opened from.
    pub from: String,
    /// sluice's check on `head`, as last read; `None` where none stood.
    pub check: Option<Posted>,
}

/// The host's `forge/` directory.
#[derive(Clone, Debug)]
pub struct Kept {
    dir: PathBuf,
}

impl Kept {
    /// The `forge/` directory under the host's state directory `state`.
    pub fn new(state: &Path) -> Kept {
        Kept {
            dir: state.join("forge"),
        }
    }

    fn answer_path(&self, url: &str) -> PathBuf {
        let hex: String = Sha256::digest(url.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.dir
            .join("answers")
            .join(format!("{}.json", &hex[..32]))
    }

    fn pause_path(&self, a: &Allowance) -> PathBuf {
        self.dir
            .join("pauses")
            .join(format!("{}-{}-{}.json", a.app, a.owner, a.resource))
    }

    /// The answer kept for `url`; `None` when none is, or it is torn, or
    /// it is another address's.
    pub fn answer(&self, url: &str) -> Option<Answer> {
        read::<Answer>(&self.answer_path(url)).filter(|a| a.url == url)
    }

    /// Keep `answer` for its address, and look over the unused ones when
    /// an hour has passed since they were last looked over.
    pub fn keep(&self, answer: &Answer) -> Result<()> {
        write(&self.answer_path(&answer.url), answer)?;
        self.sweep(SystemTime::now())
    }

    /// The kept answer for `url` was just used: GitHub said it stands.
    pub fn used(&self, url: &str) {
        let _ = std::fs::File::options()
            .write(true)
            .open(self.answer_path(url))
            .and_then(|f| f.set_modified(SystemTime::now()));
    }

    /// GitHub has nothing at `url`: what was kept for it is removed.
    pub fn forget(&self, url: &str) {
        let _ = std::fs::remove_file(self.answer_path(url));
    }

    /// Remove the answers not used for a day before `now`, unless they
    /// were looked over within the hour.
    fn sweep(&self, now: SystemTime) -> Result<()> {
        let dir = self.dir.join("answers");
        let swept = dir.join(SWEPT);
        let since = |p: &Path| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| now.duration_since(t).unwrap_or_default())
        };
        if since(&swept).is_some_and(|d| d < SWEEP) {
            return Ok(());
        }
        std::fs::write(&swept, b"").with_context(|| format!("writing {}", swept.display()))?;
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && since(&path).is_some_and(|d| d >= UNUSED)
            {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// Where `repo`'s readings of `base` are kept, before `.json`,
    /// `.lock` or `.moved`.
    fn requests_path(&self, repo: &Repo, base: &str, ext: &str) -> PathBuf {
        self.dir
            .join("requests")
            .join(&repo.owner)
            .join(&repo.name)
            .join(format!("{}.{ext}", crate::runs::flat_name(base)))
    }

    /// `repo`'s readings of `base`; empty when none are kept, or they are
    /// torn.
    pub fn readings(&self, repo: &Repo, base: &str) -> Readings {
        read(&self.requests_path(repo, base, "json")).unwrap_or_default()
    }

    /// Change `repo`'s readings of `base` by `f`, under their lock, and
    /// keep what it leaves.
    pub fn change_readings<T>(
        &self,
        repo: &Repo,
        base: &str,
        f: impl FnOnce(&mut Readings) -> T,
    ) -> Result<T> {
        let path = self.requests_path(repo, base, "lock");
        let dir = path.parent().expect("a kept file has a directory");
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| path.display().to_string())?;
        lock.lock().with_context(|| path.display().to_string())?;
        let mut readings = self.readings(repo, base);
        let out = f(&mut readings);
        write(&self.requests_path(repo, base, "json"), &readings)?;
        Ok(out)
    }

    /// A submit on the host opened, updated or marked ready a pull request
    /// of `repo` into `base`: the next pass asks at once.
    pub fn moved(&self, repo: &Repo, base: &str) -> Result<()> {
        let path = self.requests_path(repo, base, "moved");
        let dir = path.parent().expect("a kept file has a directory");
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        std::fs::File::create(&path)
            .and_then(|f| f.set_modified(SystemTime::now()))
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Whether a submit on the host moved a request of `repo` into `base`
    /// after `asked`, or ever, where it was never asked.
    pub fn moved_since(&self, repo: &Repo, base: &str, asked: Option<DateTime<Utc>>) -> bool {
        let Ok(at) =
            std::fs::metadata(self.requests_path(repo, base, "moved")).and_then(|m| m.modified())
        else {
            return false;
        };
        asked.is_none_or(|asked| DateTime::<Utc>::from(at) > asked)
    }

    /// Where the pull requests into `base` of `repo` the host follows are
    /// kept, before `.json` or `.lock`.
    fn follows_path(&self, repo: &Repo, base: &str, ext: &str) -> PathBuf {
        self.dir
            .join("follows")
            .join(&repo.owner)
            .join(&repo.name)
            .join(format!("{}.{ext}", crate::runs::flat_name(base)))
    }

    /// 137: the pull requests into `base` of `repo` the host follows; none
    /// when none are kept, or the file is torn.
    pub fn follows(&self, repo: &Repo, base: &str) -> Follows {
        read(&self.follows_path(repo, base, "json")).unwrap_or_default()
    }

    /// 137, design D5: when the pull requests into `base` of `repo` the
    /// host follows were last written, so a reader's document carries them
    /// again only once it moved; `None` where none are kept.
    pub fn follows_modified(&self, repo: &Repo, base: &str) -> Option<SystemTime> {
        std::fs::metadata(self.follows_path(repo, base, "json"))
            .and_then(|m| m.modified())
            .ok()
    }

    /// 137: change the pull requests into `base` of `repo` the host follows
    /// by `f`, under their lock, and keep what it leaves, written whole.
    pub fn change_follows<T>(
        &self,
        repo: &Repo,
        base: &str,
        f: impl FnOnce(&mut Follows) -> T,
    ) -> Result<T> {
        let path = self.follows_path(repo, base, "lock");
        let dir = path.parent().expect("a kept file has a directory");
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| path.display().to_string())?;
        lock.lock().with_context(|| path.display().to_string())?;
        let mut follows = self.follows(repo, base);
        let out = f(&mut follows);
        write(&self.follows_path(repo, base, "json"), &follows)?;
        Ok(out)
    }

    /// The pause kept for `allowance`, standing or past; `None` when none
    /// is, or it is torn.
    pub fn pause(&self, allowance: &Allowance) -> Option<Pause> {
        read(&self.pause_path(allowance))
    }

    /// Keep `pause` for `allowance`, keeping the later end of it and the
    /// one kept already: what is kept after.
    pub fn put_pause(&self, allowance: &Allowance, pause: Pause) -> Result<Pause> {
        let pause = match self.pause(allowance) {
            Some(kept) if kept.until > pause.until => Pause {
                streak: pause.streak,
                ..kept
            },
            _ => pause,
        };
        write(&self.pause_path(allowance), &pause)?;
        Ok(pause)
    }
}

/// `path`'s JSON; `None` when it is absent or does not parse.
fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// `value` written whole to `path` through a file of this process's own.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().expect("a kept file has a directory");
    std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    let name = path.file_name().unwrap().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(value)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sluice-kept-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn answer(url: &str, body: &str) -> Answer {
        Answer {
            url: url.into(),
            etag: Some(format!("\"{body}\"")),
            last_modified: Some("Wed, 28 Oct 2026 12:00:00 GMT".into()),
            body: body.into(),
            at: Utc::now(),
        }
    }

    #[test]
    fn an_answer_is_kept_and_read_back_by_its_address() {
        let dir = scratch("answer");
        let kept = Kept::new(&dir);
        let url = "https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1";
        assert_eq!(kept.answer(url), None);
        let a = answer(url, "[]");
        kept.keep(&a).unwrap();
        assert_eq!(kept.answer(url), Some(a));
        assert_eq!(kept.answer(&url.replace("page=1", "page=2")), None);
        kept.forget(url);
        assert_eq!(kept.answer(url), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_file_reads_as_absent() {
        let dir = scratch("torn");
        let kept = Kept::new(&dir);
        let url = "https://api.github.com/app";
        kept.keep(&answer(url, "{}")).unwrap();
        std::fs::write(kept.answer_path(url), b"{\"url\": \"https://api.gi").unwrap();
        assert_eq!(kept.answer(url), None);
        let a = Allowance {
            app: 1,
            owner: "o".into(),
            resource: "core".into(),
        };
        std::fs::create_dir_all(kept.pause_path(&a).parent().unwrap()).unwrap();
        std::fs::write(kept.pause_path(&a), b"{\"until\"").unwrap();
        assert_eq!(kept.pause(&a), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_answer_unused_for_a_day_is_removed_and_a_fresh_one_kept() {
        let dir = scratch("sweep");
        let kept = Kept::new(&dir);
        let (old, fresh) = ("https://api.github.com/old", "https://api.github.com/fresh");
        kept.keep(&answer(old, "old")).unwrap();
        kept.keep(&answer(fresh, "fresh")).unwrap();
        let now = SystemTime::now();
        let aged = now - UNUSED - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(kept.answer_path(old))
            .unwrap()
            .set_modified(aged)
            .unwrap();
        // Looked over within the hour: nothing is removed.
        kept.sweep(now).unwrap();
        assert!(kept.answer(old).is_some());
        // An hour on, the old one goes and the fresh one stays.
        kept.sweep(now + SWEEP).unwrap();
        assert_eq!(kept.answer(old), None);
        assert!(kept.answer(fresh).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 136: a branch's readings are kept under their lock and read back,
    /// each branch apart, and torn ones read as none.
    #[test]
    fn readings_are_kept_under_their_lock_and_read_back() {
        let dir = scratch("readings");
        let kept = Kept::new(&dir);
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        assert_eq!(kept.readings(&repo, "main"), Readings::default());
        let asked = Utc::now();
        let reading = Reading {
            head: "abc".into(),
            draft: true,
            missing: Some(vec!["a draft".into()]),
            ..Reading::default()
        };
        let back = kept
            .change_readings(&repo, "bolt/x", |r| {
                r.asked = Some(asked);
                r.tip = Some("t1".into());
                r.version = Some(4);
                r.requests.insert(7, reading.clone());
                r.requests.len()
            })
            .unwrap();
        assert_eq!(back, 1);
        let r = kept.readings(&repo, "bolt/x");
        assert_eq!(
            (r.asked, r.tip.as_deref(), r.version),
            (Some(asked), Some("t1"), Some(4))
        );
        assert_eq!(r.requests[&7], reading);
        assert_eq!(kept.readings(&repo, "bolt"), Readings::default());
        assert!(dir.join("forge/requests/o/r/bolt%2Fx.lock").exists());
        // The lock is free once the change is kept: another takes it.
        kept.change_readings(&repo, "bolt/x", |r| r.requests.clear())
            .unwrap();
        assert!(kept.readings(&repo, "bolt/x").requests.is_empty());
        std::fs::write(dir.join("forge/requests/o/r/bolt%2Fx.json"), b"{\"asked\"").unwrap();
        assert_eq!(kept.readings(&repo, "bolt/x"), Readings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 136: a submit's touch newer than the last asking asks at once; one
    /// older does not, and none never does.
    #[test]
    fn a_move_newer_than_the_asking_asks_at_once() {
        let dir = scratch("moved");
        let kept = Kept::new(&dir);
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        let before = Utc::now() - TimeDelta::seconds(5);
        assert!(!kept.moved_since(&repo, "main", Some(before)));
        assert!(!kept.moved_since(&repo, "main", None));
        kept.moved(&repo, "main").unwrap();
        assert!(kept.moved_since(&repo, "main", Some(before)));
        assert!(kept.moved_since(&repo, "main", None));
        let after = Utc::now() + TimeDelta::seconds(5);
        assert!(!kept.moved_since(&repo, "main", Some(after)));
        assert!(!kept.moved_since(&repo, "bolt/x", Some(before)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 137: a followed request is kept under its lock with what was read
    /// of it, read back, each branch apart, and leaves when taken out; a
    /// torn file reads as none.
    #[test]
    fn a_followed_request_is_kept_and_leaves() {
        let dir = scratch("follows");
        let kept = Kept::new(&dir);
        let repo = Repo::from_url("https://github.com/o/r.git").unwrap();
        assert_eq!(kept.follows(&repo, "main"), Follows::default());
        let check = Posted {
            id: 9,
            name: "sluice".into(),
            posting: super::super::forge::Posting {
                status: super::super::forge::Status::Queued,
                title: "queued: next on bolt/x".into(),
                summary: "Submission 4 on b.".into(),
                text: None,
            },
        };
        let follow = Follow {
            read_at: Some(Utc::now()),
            head: "abc".into(),
            open: true,
            merged: false,
            draft: false,
            title: "Add the thing".into(),
            from: "unit/thing".into(),
            check: Some(check),
        };
        let kept_now = kept
            .change_follows(&repo, "bolt/x", |f| {
                f.requests.insert(12, follow.clone());
                f.requests.len()
            })
            .unwrap();
        assert_eq!(kept_now, 1);
        assert_eq!(kept.follows(&repo, "bolt/x").requests[&12], follow);
        assert_eq!(kept.follows(&repo, "bolt"), Follows::default());
        assert!(dir.join("forge/follows/o/r/bolt%2Fx.json").exists());
        assert!(dir.join("forge/follows/o/r/bolt%2Fx.lock").exists());
        // The lock is free once the change is kept: another takes it.
        kept.change_follows(&repo, "bolt/x", |f| f.requests.remove(&12))
            .unwrap();
        assert!(kept.follows(&repo, "bolt/x").requests.is_empty());
        std::fs::write(
            dir.join("forge/follows/o/r/bolt%2Fx.json"),
            b"{\"requests\"",
        )
        .unwrap();
        assert_eq!(kept.follows(&repo, "bolt/x"), Follows::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pause_keeps_the_later_end() {
        let dir = scratch("pause");
        let kept = Kept::new(&dir);
        let a = Allowance {
            app: 4242,
            owner: "o".into(),
            resource: "core".into(),
        };
        let now = Utc::now();
        let later = Pause {
            until: now + TimeDelta::minutes(20),
            words: "API rate limit exceeded".into(),
            streak: 0,
        };
        let sooner = Pause {
            until: now + TimeDelta::minutes(1),
            words: "You have exceeded a secondary rate limit".into(),
            streak: 1,
        };
        assert_eq!(kept.put_pause(&a, later.clone()).unwrap(), later);
        let after = kept.put_pause(&a, sooner.clone()).unwrap();
        assert_eq!(after.until, later.until);
        assert_eq!(after.streak, 1);
        assert_eq!(kept.pause(&a), Some(after));
        let graphql = Allowance {
            resource: "graphql".into(),
            ..a.clone()
        };
        assert_eq!(kept.pause(&graphql), None, "each allowance its own");
        let even_later = Pause {
            until: now + TimeDelta::minutes(30),
            ..sooner
        };
        assert_eq!(kept.put_pause(&a, even_later.clone()).unwrap(), even_later);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The forge-allowance spec's "A command's token honours the pause": a
    /// pause is kept under the app, the repository's owner and the
    /// allowance, whichever way the token came (134; record 0108).
    #[test]
    fn a_pause_is_kept_under_the_owner() {
        let dir = scratch("owner");
        let kept = Kept::new(&dir);
        let a = Allowance {
            app: 4242,
            owner: "o".into(),
            resource: "core".into(),
        };
        let pause = Pause {
            until: Utc::now() + TimeDelta::minutes(20),
            words: "API rate limit exceeded".into(),
            streak: 0,
        };
        kept.put_pause(&a, pause.clone()).unwrap();
        assert!(dir.join("forge/pauses/4242-o-core.json").is_file());
        assert_eq!(kept.pause(&a), Some(pause));
        let other = Allowance {
            owner: "p".into(),
            ..a.clone()
        };
        assert_eq!(kept.pause(&other), None);
        let graphql = Allowance {
            resource: "graphql".into(),
            ..a
        };
        assert_eq!(kept.pause(&graphql), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
