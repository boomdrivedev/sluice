//! The `pushed refs` profile and the router that chooses it, against a
//! bare repository as the remote, which git treats as it treats a forge
//! (C.4, C.7 `remote`). The check-tools tier: run with `--features tools`.

#![cfg(feature = "tools")]

mod support;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::time::{Duration, Instant};

use sluice::identity::{Credentials, Identity, Withheld};
use sluice::profiles::branches::Branches;
use sluice::profiles::pushed_refs::PushedRefs;
use sluice::retry;
use sluice_core::policy::Policy;
use sluice_seams::{read_refused, Branch, HostCopy, LandingCommit, Moved, ReadRefused, Rev};
use support::http::{
    commands, empty_repository, is_push, listing, lists_for_push, report, takes, unavailable,
    Answer, Count, Fake, Request, Silent, CUT,
};
use support::{soon, Repo, Scratch};

fn rev(s: String) -> Rev {
    Rev::new(s)
}

/// The pushed refs of `repo`'s `origin`, on the host's own credentials,
/// its retry policy's pauses skipped.
fn open(repo: &Repo) -> PushedRefs {
    PushedRefs::open_as(repo.dir(), "origin", None, recorded()).unwrap()
}

/// A landing commit of `head` onto `onto`, as the core writes one.
fn landing(b: &impl Branch, onto: &Rev, head: &Rev) -> Rev {
    let Ok(sluice_seams::Formed::Revision(c)) = b.form(onto, head) else {
        panic!("formed no candidate");
    };
    b.commit(&LandingCommit {
        parent: onto.clone(),
        head: head.clone(),
        tree_of: c,
        message: "land\n".into(),
    })
    .unwrap()
}

/// 42: a read sees what another clone pushed, and moves no local branch
/// (rule core.4).
#[test]
fn a_read_sees_what_another_clone_pushed() {
    let repo = Repo::new("read");
    let bare = repo.bare_origin();
    let before = repo.rev("main");
    let other = Repo::clone_of(bare.path(), "other");
    let pushed = other.push_change("b", "2\n");
    let p = open(&repo);
    assert_eq!(p.read("main").unwrap(), Some(rev(pushed.clone())));
    assert_eq!(repo.rev("main"), before);
    assert_eq!(repo.rev("refs/remotes/origin/main"), pushed);
    assert_eq!(p.read("none").unwrap(), None);
}

/// Rules core.28, core.29: one fetch at a time in a repository. A fetch
/// waits while another process holds the repository's fetch lock, and
/// reads the branch once it is let go.
#[test]
fn a_fetch_waits_for_another_fetch_of_its_repository() {
    let repo = Repo::new("fetch-lock");
    let bare = repo.bare_origin();
    let other = Repo::clone_of(bare.path(), "fetch-lock-other");
    let pushed = other.push_change("b", "2\n");
    let lock = std::fs::File::create(repo.dir().join(".git/sluice-fetch.lock")).unwrap();
    lock.lock().unwrap();
    let dir = repo.dir().to_path_buf();
    let reader = std::thread::spawn(move || {
        let p = PushedRefs::open_as(&dir, "origin", None, recorded()).unwrap();
        p.read("main").map_err(|e| format!("{e:#}"))
    });
    std::thread::sleep(Duration::from_millis(500));
    assert!(!reader.is_finished(), "the fetch ran beside another");
    drop(lock);
    assert_eq!(reader.join().unwrap(), Ok(Some(rev(pushed))));
}

/// 134, rule core.29, record 0109: a fetch the remote never answers is
/// stopped at the limit of a try, with the remote's helper, and tried
/// again; once the policy's time is spent it is a refused read in
/// sluice's words, and the repository's fetch lock is free. Before the
/// limit, the first such fetch held the lock for as long as the remote
/// kept the connection, and every other read of the repository queued
/// behind it (deneb, 2026-11-10: sluice 0.73.1's `status <branch> --all
/// --json` and `overview --json` hanging 10 to 17 minutes).
#[test]
fn a_fetch_the_remote_never_answers_is_stopped_and_lets_its_lock_go() {
    let repo = Repo::new("fetch-silent");
    let _bare = repo.bare_origin();
    let silent = Silent::start();
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        &format!("{}/o/r.git", silent.url()),
    ]);
    let p = PushedRefs::open_as(
        repo.dir(),
        "origin",
        None,
        trying(Duration::from_millis(300)),
    )
    .unwrap();
    let started = Instant::now();
    let err = p.read("main").unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(20), "{err:#}");
    let refused = read_refused(&err).expect("a refused read");
    assert!(
        refused
            .said
            .ends_with("git fetch with origin was still going after 300ms, the most a try may take, and was stopped"),
        "{}",
        refused.said
    );
    assert!(silent.requests().len() > 1, "tried again");
    assert!(silent.all_let_go(), "a helper outlived its try");
    // A git another test of this binary starts meanwhile may hold the
    // lock for a moment, until it runs its program (deneb, 2026-11-11: the
    // lock read held at once after the read returned).
    let lock = std::fs::File::open(repo.dir().join(".git/sluice-fetch.lock")).unwrap();
    assert!(
        soon(|| lock.try_lock().is_ok()),
        "the fetch lock outlived the fetch"
    );
}

/// Rules core.28, core.29, record 0109: a fetch waits for the repository's
/// fetch lock no longer than a try may take, and fails, as no read the
/// remote refused, when another fetch held it every time it tried; once
/// the lock is let go it reads.
#[test]
fn a_fetch_waits_for_the_lock_no_longer_than_a_try() {
    let repo = Repo::new("fetch-lock-held");
    let bare = repo.bare_origin();
    let other = Repo::clone_of(bare.path(), "fetch-lock-held-other");
    let pushed = other.push_change("b", "2\n");
    let lock = std::fs::File::create(repo.dir().join(".git/sluice-fetch.lock")).unwrap();
    lock.lock().unwrap();
    let p = PushedRefs::open_as(
        repo.dir(),
        "origin",
        None,
        trying(Duration::from_millis(300)),
    )
    .unwrap();
    let started = Instant::now();
    let err = p.read("main").unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(20), "{err:#}");
    assert!(read_refused(&err).is_none(), "{err:#}");
    assert!(
        format!("{err:#}").ends_with("sluice-fetch.lock past 300ms, the most a try waits for it"),
        "{err:#}"
    );
    drop(lock);
    assert_eq!(p.read("main").unwrap(), Some(rev(pushed)));
}

/// 122, 134, rule core.29, record 0109: a push the remote never answers is
/// stopped at the limit of a try and never pushed again, since whether the
/// remote took it is not known; what the remote holds is read, and a
/// remote that answers that read no more fails the move, saying so.
#[test]
fn a_push_the_remote_never_answers_is_stopped_and_not_pushed_again() {
    let repo = Repo::new("push-silent");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let to = landing(&p, &rev(base.clone()), &head);
    let silent = Silent::start();
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        &format!("{}/o/r.git", silent.url()),
    ]);
    let p = PushedRefs::open_as(
        repo.dir(),
        "origin",
        None,
        trying(Duration::from_millis(300)),
    )
    .unwrap();
    let started = Instant::now();
    let err = format!("{:#}", p.move_to("main", &rev(base), &to).unwrap_err());
    assert!(started.elapsed() < Duration::from_secs(20), "{err}");
    assert!(
        err.contains("git push with origin was still going after 300ms"),
        "{err}"
    );
    assert!(err.contains("the push went unanswered"), "{err}");
    let pushes = silent
        .requests()
        .iter()
        .filter(|r| r.path.contains("git-receive-pack"))
        .count();
    assert_eq!(pushes, 1, "pushed again");
    assert!(silent.all_let_go(), "a helper outlived its try");
}

/// Rules core.28, core.29: a fetch that loses a race for a ref of this
/// repository's, as one beside a fetch that holds no lock does, is no
/// refused read: the branch reads as that fetch left it, and the next
/// fetch brings it.
#[test]
fn a_race_lost_on_a_fetched_ref_is_no_refused_read() {
    let repo = Repo::new("fetch-race");
    let bare = repo.bare_origin();
    let p = open(&repo);
    let before = p.read("main").unwrap().unwrap();
    let other = Repo::clone_of(bare.path(), "fetch-race-other");
    let pushed = other.push_change("b", "2\n");
    let held = repo.dir().join(".git/refs/remotes/origin/main.lock");
    std::fs::write(&held, format!("{before}\n")).unwrap();
    let read = p.read("main");
    std::fs::remove_file(&held).unwrap();
    assert_eq!(read.map_err(|e| format!("{e:#}")), Ok(Some(before)));
    assert_eq!(p.read("main").unwrap(), Some(rev(pushed)));
}

/// 45, 46, 124: a push the lease allows lands on the remote, and leaves
/// the host's copy where it was, for a round to level.
#[test]
fn a_push_lands_and_leaves_the_copy() {
    let repo = Repo::new("push");
    let bare = repo.bare_origin();
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let base = p.read("main").unwrap().unwrap();
    let to = landing(&p, &base, &head);
    assert_eq!(p.move_to("main", &base, &to).unwrap(), Moved::Moved);
    let remote = Repo::clone_of(bare.path(), "check");
    assert_eq!(remote.rev("origin/main"), to.as_str());
    assert_eq!(repo.rev("main"), base.as_str());
}

/// 122, 124: a push accepted while git holds the copy's lock is a
/// landing all the same; the copy is left where it was.
#[test]
fn a_push_accepted_under_the_copys_lock_is_moved() {
    let repo = Repo::new("push-locked");
    let bare = repo.bare_origin();
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let base = p.read("main").unwrap().unwrap();
    let to = landing(&p, &base, &head);
    let lock = repo.dir().join(".git/refs/heads/main.lock");
    std::fs::write(&lock, "").unwrap();
    assert_eq!(p.move_to("main", &base, &to).unwrap(), Moved::Moved);
    std::fs::remove_file(&lock).unwrap();
    let remote = Repo::clone_of(bare.path(), "check");
    assert_eq!(remote.rev("origin/main"), to.as_str());
    assert_eq!(repo.rev("main"), base.as_str());
}

/// 24, rule core.5: a push after another clone moved the branch is
/// refused and reports what the remote holds.
#[test]
fn a_push_after_the_branch_moved_is_elsewhere() {
    let repo = Repo::new("moved");
    let bare = repo.bare_origin();
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let base = p.read("main").unwrap().unwrap();
    let to = landing(&p, &base, &head);
    let other = Repo::clone_of(bare.path(), "other");
    let moved = other.push_change("b", "2\n");
    assert_eq!(
        p.move_to("main", &base, &to).unwrap(),
        Moved::Elsewhere(Some(rev(moved.clone())))
    );
    let remote = Repo::clone_of(bare.path(), "check");
    assert_eq!(remote.rev("origin/main"), moved);
    assert_eq!(repo.rev("main"), base.as_str());
}

/// A remote that cannot be reached: the read and the push fail, and the
/// push moves nothing.
#[test]
fn an_unreachable_remote_fails_the_read_and_the_push() {
    let repo = Repo::new("unreachable");
    let bare = repo.bare_origin();
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let base = p.read("main").unwrap().unwrap();
    let to = landing(&p, &base, &head);
    let aside = bare.path().with_extension("aside");
    std::fs::rename(bare.path(), &aside).unwrap();
    let read = p.read("main").unwrap_err().to_string();
    assert!(read.contains("fetching main from origin failed"), "{read}");
    let push = p.move_to("main", &base, &to).unwrap_err().to_string();
    assert!(push.contains("pushing main to origin failed"), "{push}");
    std::fs::rename(&aside, bare.path()).unwrap();
    assert_eq!(repo.rev("main"), base.as_str());
    assert_eq!(p.read("main").unwrap(), Some(base));
}

/// 133: a fetch from a remote whose path is not there fails as a refused
/// read naming the remote, the ref asked for and git's words unchanged, as
/// git prints them for the same fetch.
#[test]
fn a_fetch_from_a_remote_not_there_is_a_refused_read() {
    let repo = Repo::new("not-there");
    let bare = repo.bare_origin();
    let p = open(&repo);
    std::fs::rename(bare.path(), bare.path().with_extension("aside")).unwrap();
    let err = p.read("main").unwrap_err();
    let refused = read_refused(&err).expect("a refused read");
    let git = repo.git_raw(&[
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-write-fetch-head",
        "origin",
        "+refs/heads/main:refs/remotes/origin/main",
    ]);
    assert!(!git.status.success());
    assert_eq!(
        refused,
        &ReadRefused {
            remote: "origin".into(),
            asked: "refs/heads/main".into(),
            said: String::from_utf8_lossy(&git.stderr).trim_end().into(),
            note: None,
        }
    );
    assert!(refused
        .said
        .contains("does not appear to be a git repository"));
    assert_eq!(
        err.to_string(),
        format!("fetching main from origin failed: {}", refused.said)
    );
}

/// A push the remote refuses names the remote's reason, which git's own
/// error leaves out: a forge refusing an app's change to its workflows
/// says so only there.
#[test]
fn a_refused_push_names_the_remotes_reason() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new("refused");
    let bare = repo.bare_origin();
    let hook = bare.path().join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let p = open(&repo);
    let base = p.read("main").unwrap().unwrap();
    let err = p.push_branch(&head, "unit/u").unwrap_err().to_string();
    assert!(err.contains("pre-receive hook declined"), "{err}");
    let to = landing(&p, &base, &head);
    let err = p.move_to("main", &base, &to).unwrap_err().to_string();
    assert!(err.contains("pre-receive hook declined"), "{err}");
    assert_eq!(repo.rev("main"), base.as_str());
}

/// 124: the host's copy is its own `refs/heads/<branch>`, read with no
/// fetch, so with the remote unreachable.
#[test]
fn the_copy_is_read_with_the_remote_unreachable() {
    let repo = Repo::new("copy-read");
    let bare = repo.bare_origin();
    let base = rev(repo.rev("main"));
    let other = Repo::clone_of(bare.path(), "other");
    other.push_change("b", "2\n");
    let p = open(&repo);
    let aside = bare.path().with_extension("aside");
    std::fs::rename(bare.path(), &aside).unwrap();
    assert_eq!(p.copy("main").unwrap(), HostCopy::At(Some(base)));
    assert_eq!(p.copy("none").unwrap(), HostCopy::At(None));
    std::fs::rename(&aside, bare.path()).unwrap();
}

/// 124, rule core.5: a levelling from the value the copy holds sets it,
/// with sluice's reflog line; one from any other value, or from none
/// while the copy exists, is reported with what it holds and moves
/// nothing.
#[test]
fn a_copy_is_levelled_only_from_the_value_it_holds() {
    let repo = Repo::new("copy-level");
    let bare = repo.bare_origin();
    let base = rev(repo.rev("main"));
    let stale = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let other = Repo::clone_of(bare.path(), "other");
    let pushed = rev(other.push_change("b", "2\n"));
    let p = open(&repo);
    assert_eq!(p.read("main").unwrap(), Some(pushed.clone()));
    assert_eq!(
        p.level("main", Some(&stale), &pushed).unwrap(),
        Moved::Elsewhere(Some(base.clone()))
    );
    assert_eq!(
        p.level("main", None, &pushed).unwrap(),
        Moved::Elsewhere(Some(base.clone()))
    );
    assert_eq!(repo.rev("main"), base.as_str());
    assert_eq!(p.level("main", Some(&base), &pushed).unwrap(), Moved::Moved);
    assert_eq!(repo.rev("main"), pushed.as_str());
    assert_eq!(
        repo.git(&["reflog", "show", "-1", "--format=%gs", "refs/heads/main"]),
        "sluice: level main to its landing"
    );
    assert_eq!(p.level("bolt/y", None, &pushed).unwrap(), Moved::Moved);
    assert_eq!(repo.rev("bolt/y"), pushed.as_str());
}

/// 124: a levelling while git holds the copy's lock fails, saying so, and
/// moves nothing.
#[test]
fn a_levelling_fails_on_the_copys_lock() {
    let repo = Repo::new("copy-lock");
    let bare = repo.bare_origin();
    let base = rev(repo.rev("main"));
    let other = Repo::clone_of(bare.path(), "other");
    let pushed = rev(other.push_change("b", "2\n"));
    let p = open(&repo);
    p.read("main").unwrap();
    let lock = repo.dir().join(".git/refs/heads/main.lock");
    std::fs::write(&lock, "").unwrap();
    let err = format!("{:#}", p.level("main", Some(&base), &pushed).unwrap_err());
    assert!(
        err.contains("git holds the lock on refs/heads/main"),
        "{err}"
    );
    assert_eq!(repo.rev("main"), base.as_str());
    std::fs::remove_file(&lock).unwrap();
    assert_eq!(p.level("main", Some(&base), &pushed).unwrap(), Moved::Moved);
}

/// C.7, 124: the router sends `copy` and `level` where it sends `read`.
#[test]
fn the_router_sends_the_copy_to_the_branchs_profile() {
    let repo = Repo::new("router-copy");
    repo.write(
        ".config/sluice.toml",
        "[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x"]);
    let _bare = repo.bare_origin();
    let base = rev(repo.rev("main"));
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.copy("main").unwrap(), HostCopy::At(Some(base.clone())));
    assert_eq!(b.copy("bolt/x").unwrap(), HostCopy::Itself);
    assert!(b.level("bolt/x", Some(&base), &base).is_err());
}

/// C.7: a branch with no `remote` is local refs, exactly as before.
#[test]
fn the_router_keeps_a_branch_without_a_remote_local() {
    let repo = Repo::new("router-local");
    repo.write(".config/sluice.toml", "[branches.main]\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let bare = repo.bare_origin();
    let base = rev(repo.rev("main"));
    let other = Repo::clone_of(bare.path(), "other");
    other.push_change("b", "2\n");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.read("main").unwrap(), Some(base.clone()));
    let to = landing(&b, &base, &head);
    assert_eq!(b.move_to("main", &base, &to).unwrap(), Moved::Moved);
    assert_eq!(repo.rev("main"), to.as_str());
    assert_ne!(
        Repo::clone_of(bare.path(), "check").rev("origin/main"),
        to.as_str()
    );
}

/// C.7: a branch whose `remote` names a remote reads and lands through
/// it; one naming a remote the repository lacks is refused by name.
#[test]
fn the_router_sends_a_branch_to_its_remote() {
    let repo = Repo::new("router-remote");
    repo.write(
        ".config/sluice.toml",
        "[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\nremote = \"nowhere\"\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x"]);
    let bare = repo.bare_origin();
    let other = Repo::clone_of(bare.path(), "other");
    let pushed = other.push_change("b", "2\n");
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.read("main").unwrap(), Some(rev(pushed)));
    let err = format!("{:#}", b.read("bolt/x").unwrap_err());
    assert!(err.contains("\"nowhere\""), "{err}");
}

/// A repository whose `origin/main`, as fetched, holds `policy`, while
/// the host's own `main` is the root commit, from before the policy file.
/// Returns the root and origin's revision.
fn a_copy_from_before_the_policy(repo: &Repo, policy: &str) -> (Scratch, String, String) {
    let root = repo.rev("main");
    repo.write(".config/sluice.toml", policy);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let bare = repo.bare_origin();
    let fetched = repo.rev("refs/remotes/origin/main");
    repo.git(&["reset", "-q", "--hard", &root]);
    (bare, root, fetched)
}

/// The revision `policy_of` gave, and the remote its policy names for
/// `branch`.
fn routed_by(at: Option<(Rev, Policy)>, branch: &str) -> (String, Option<String>) {
    let (rev, policy) = at.expect("a policy to route by");
    let remote = policy
        .rules_for(branch)
        .unwrap()
        .and_then(|r| r.remote.clone());
    (rev.to_string(), remote)
}

/// 124: a host whose own `main` is from before the policy file routes
/// `main` by origin's copy as last fetched, and reads origin's revision;
/// its own `main` stays where it was.
#[test]
fn the_router_reads_a_copy_from_before_the_policy_as_origins() {
    let repo = Repo::new("router-stale");
    let (_bare, root, fetched) =
        a_copy_from_before_the_policy(&repo, "[branches.main]\nremote = \"origin\"\n");
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap().as_deref(), Some("origin"));
    assert_eq!(b.read("main").unwrap(), Some(rev(fetched)));
    assert_eq!(repo.rev("main"), root);
}

/// 124: a branch to be made from a `main` this host holds from before
/// the policy file, and the policy `main`'s records are routed by, are
/// read off origin's copy as `remote` reads it.
#[test]
fn made_from_and_policy_of_read_a_copy_from_before_the_policy_as_origins() {
    let repo = Repo::new("router-stale-made");
    let (_bare, _, fetched) = a_copy_from_before_the_policy(
        &repo,
        "[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\nremote = \"origin\"\n",
    );
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(
        b.remote_if_made_from("bolt/x", "main").unwrap().as_deref(),
        Some("origin")
    );
    assert_eq!(
        routed_by(b.policy_of("main").unwrap(), "main"),
        (fetched, Some("origin".into()))
    );
}

/// 124: a host's `main` whose policy names no remote yet decides nothing
/// against origin's later copy, which names origin.
#[test]
fn the_router_follows_a_later_copy_naming_a_remote() {
    let repo = Repo::new("router-later");
    repo.write(".config/sluice.toml", "[branches.main]\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let bare = repo.bare_origin();
    let own = repo.rev("main");
    let other = Repo::clone_of(bare.path(), "other");
    let named = other.push_change(
        ".config/sluice.toml",
        "[branches.main]\nremote = \"origin\"\n",
    );
    repo.git(&["fetch", "-q", "origin"]);
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap().as_deref(), Some("origin"));
    assert_eq!(
        routed_by(b.policy_of("main").unwrap(), "main"),
        (named.clone(), Some("origin".into()))
    );
    assert_eq!(b.read("main").unwrap(), Some(rev(named)));
    assert_eq!(repo.rev("main"), own);
}

/// 124: with neither the host's `main` nor origin's copy naming a remote,
/// `main` stays in local refs, read from the host's own ref.
#[test]
fn the_router_keeps_a_branch_no_copy_routes_local() {
    let repo = Repo::new("router-none");
    repo.write(".config/sluice.toml", "[branches.main]\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let bare = repo.bare_origin();
    let own = repo.rev("main");
    let other = Repo::clone_of(bare.path(), "other");
    let theirs = other.push_change("b", "2\n");
    repo.git(&["fetch", "-q", "origin"]);
    assert_eq!(repo.rev("refs/remotes/origin/main"), theirs);
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap(), None);
    assert_eq!(b.read("main").unwrap(), Some(rev(own.clone())));
    assert_eq!(routed_by(b.policy_of("main").unwrap(), "main"), (own, None));
}

/// 124: the host's `main` naming origin and `upstream/main` naming
/// upstream are refused, the refusal naming each remote and its ref.
#[test]
fn the_router_refuses_two_remotes_named() {
    let repo = Repo::new("router-two");
    repo.write(
        ".config/sluice.toml",
        "[branches.main]\nremote = \"origin\"\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let _origin = repo.bare_origin();
    let upstream = Scratch::new("upstream.git");
    let at = upstream.path().to_str().unwrap();
    repo.git(&["clone", "-q", "--bare", ".", at]);
    repo.git(&["remote", "add", "upstream", at]);
    let theirs = repo.branch(
        "upstream-policy",
        "main",
        &[(
            ".config/sluice.toml",
            Some("[branches.main]\nremote = \"upstream\"\n"),
        )],
    );
    repo.git(&[
        "push",
        "-q",
        "upstream",
        &format!("{theirs}:refs/heads/main"),
    ]);
    repo.git(&["fetch", "-q", "upstream"]);
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    for err in [
        format!("{:#}", b.remote("main").unwrap_err()),
        format!("{:#}", b.read("main").unwrap_err()),
    ] {
        assert!(err.contains("origin on main ("), "{err}");
        assert!(err.contains("upstream on upstream/main ("), "{err}");
    }
}

/// 124: deciding where a branch is fetches nothing: with origin out of
/// reach, origin's copy as last fetched still routes `main` there, and
/// only the read that follows tries origin, failing.
#[test]
fn the_router_decides_with_the_remote_out_of_reach() {
    let repo = Repo::new("router-offline");
    let (_bare, _, _) =
        a_copy_from_before_the_policy(&repo, "[branches.main]\nremote = \"origin\"\n");
    let gone = repo.dir().join("no-such-remote.git");
    repo.git(&["remote", "set-url", "origin", gone.to_str().unwrap()]);
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap().as_deref(), Some("origin"));
    assert!(b.read("main").is_err());
}

/// A `[[checkout.cache]]` table, which sluice no longer keeps and whose
/// policy it refuses whole (ADR 0049).
const CACHE: &str = "[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n";

/// Commit `policy` onto the host's own `branch`, from where it is.
fn own_policy(repo: &Repo, branch: &str, policy: &str) -> String {
    repo.branch(branch, branch, &[(".config/sluice.toml", Some(policy))])
}

/// 124, record 0069: the host's own `bolt/x`, whose policy sluice refuses
/// for a `[[checkout.cache]]` table, still names origin for it, and
/// `origin/bolt/x`, clean, names it too: `bolt/x` is origin's, read from
/// origin, and deciding it says nothing of the table.
#[test]
fn a_copy_refused_elsewhere_still_says_where_the_branch_is() {
    let repo = Repo::new("router-refused-copy");
    let policy = "[branches.main]\n[branches.\"bolt/*\"]\nremote = \"origin\"\n";
    repo.write(".config/sluice.toml", policy);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x"]);
    let _bare = repo.bare_origin();
    let theirs = repo.rev("refs/remotes/origin/bolt/x");
    let own = own_policy(&repo, "bolt/x", &format!("{policy}{CACHE}"));
    assert!(Policy::parse(&format!("{policy}{CACHE}")).is_err());
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("bolt/x").unwrap().as_deref(), Some("origin"));
    assert_eq!(b.read("bolt/x").unwrap(), Some(rev(theirs)));
    assert_eq!(repo.rev("bolt/x"), own);
}

/// 124, record 0069: where the host's own `bolt/x`, refused elsewhere in
/// its policy, is the one revision naming origin, the host holding no
/// `origin/bolt/x`, `bolt/x` is still origin's, not a branch in this
/// host's own refs, and a read fetches it from origin.
#[test]
fn the_one_revision_naming_the_remote_is_refused_elsewhere() {
    let repo = Repo::new("router-refused-only");
    let policy = "[branches.main]\n[branches.\"bolt/*\"]\nremote = \"origin\"\n";
    repo.write(".config/sluice.toml", policy);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let _bare = repo.bare_origin();
    let theirs = repo.rev("main");
    repo.git(&["push", "-q", "origin", "main:refs/heads/bolt/x"]);
    repo.git(&["update-ref", "-d", "refs/remotes/origin/bolt/x"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let own = own_policy(&repo, "bolt/x", &format!("{policy}{CACHE}"));
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("bolt/x").unwrap().as_deref(), Some("origin"));
    assert_eq!(b.read("bolt/x").unwrap(), Some(rev(theirs.clone())));
    assert_eq!(repo.rev("refs/remotes/origin/bolt/x"), theirs);
    assert_eq!(repo.rev("bolt/x"), own);
}

/// 124, record 0069: a copy whose `remote` for the branch is not a
/// remote's name refuses the routing, naming the copy, its revision and
/// what `remote` must be.
#[test]
fn a_remote_that_cant_be_read_refuses_naming_the_copy() {
    let repo = Repo::new("router-unread-remote");
    repo.write(".config/sluice.toml", "[branches.main]\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let _bare = repo.bare_origin();
    repo.git(&["branch", "bolt/x", "main"]);
    let own = own_policy(
        &repo,
        "bolt/x",
        "[branches.main]\n[branches.\"bolt/*\"]\nremote = 1\n",
    );
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    for err in [
        format!("{:#}", b.remote("bolt/x").unwrap_err()),
        format!("{:#}", b.read("bolt/x").unwrap_err()),
    ] {
        assert!(err.contains("bolt/x's policy at "), "{err}");
        assert!(err.contains(&own[..7]), "{err}");
        assert!(
            err.contains("[branches.\"bolt/*\"] remote must be the name of a git remote"),
            "{err}"
        );
    }
}

/// 124, record 0069: the host's `main`, refused elsewhere in its policy,
/// naming origin, and `upstream/main` naming upstream, are refused still,
/// the refusal naming each remote and its ref.
#[test]
fn two_remotes_are_still_refused_when_one_copy_is_refused_elsewhere() {
    let repo = Repo::new("router-two-refused");
    repo.write(
        ".config/sluice.toml",
        &format!("[branches.main]\nremote = \"origin\"\n{CACHE}"),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let _origin = repo.bare_origin();
    let upstream = Scratch::new("upstream.git");
    let at = upstream.path().to_str().unwrap();
    repo.git(&["clone", "-q", "--bare", ".", at]);
    repo.git(&["remote", "add", "upstream", at]);
    let theirs = repo.branch(
        "upstream-policy",
        "main",
        &[(
            ".config/sluice.toml",
            Some("[branches.main]\nremote = \"upstream\"\n"),
        )],
    );
    repo.git(&[
        "push",
        "-q",
        "upstream",
        &format!("{theirs}:refs/heads/main"),
    ]);
    repo.git(&["fetch", "-q", "upstream"]);
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    for err in [
        format!("{:#}", b.remote("main").unwrap_err()),
        format!("{:#}", b.read("main").unwrap_err()),
    ] {
        assert!(err.contains("origin on main ("), "{err}");
        assert!(err.contains("upstream on upstream/main ("), "{err}");
        assert!(!err.contains("cache"), "{err}");
    }
}

/// 124, record 0069: the policy `main` is routed by, and the remote a
/// branch would be made on from it, are read off a copy refused elsewhere
/// in its policy as `remote` reads it.
#[test]
fn made_from_and_policy_of_read_a_refused_copy_as_remote_does() {
    let repo = Repo::new("router-refused-made");
    repo.write(
        ".config/sluice.toml",
        &format!(
            "[branches.main]\nremote = \"origin\"\n[branches.\"bolt/*\"]\nremote = \"origin\"\n\
             [signers.box]\nssh = \"keys/box.pub\"\n{CACHE}"
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let own = repo.rev("main");
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap().as_deref(), Some("origin"));
    assert_eq!(
        b.remote_if_made_from("bolt/x", "main").unwrap().as_deref(),
        Some("origin")
    );
    let at = b.policy_of("main").unwrap();
    let signers: Vec<String> = at.as_ref().unwrap().1.signers.keys().cloned().collect();
    assert_eq!(signers, ["box"]);
    assert_eq!(routed_by(at, "main"), (own, Some("origin".into())));
}

/// An identity whose remote is a fake HTTP server, with a token of its
/// own: `token`, then `token-1`, `token-2` and on as it is renewed, unless
/// its forge refuses to make another, saying `refuses`, or gives none at
/// all, as `withholds` says.
struct FakeIdentity {
    /// The URL it reaches every remote by; `None`, each remote's own.
    url: Option<String>,
    token: String,
    renewed: Cell<usize>,
    refuses: Option<String>,
    withholds: Option<Withheld>,
    /// The URL each credential was asked for, in order.
    asked: RefCell<Vec<String>>,
}

impl FakeIdentity {
    fn new(url: &str, token: &str) -> FakeIdentity {
        FakeIdentity {
            url: Some(url.into()),
            ..FakeIdentity::following(token)
        }
    }

    /// The identity that reaches each remote at its own address.
    fn following(token: &str) -> FakeIdentity {
        FakeIdentity {
            url: None,
            token: token.into(),
            renewed: Cell::new(0),
            refuses: None,
            withholds: None,
            asked: RefCell::default(),
        }
    }

    /// The identity whose forge makes no fresh token, saying `why`.
    fn refusing(self, why: &str) -> FakeIdentity {
        FakeIdentity {
            refuses: Some(why.into()),
            ..self
        }
    }

    /// The identity whose forge gives no credentials, as `withheld` says.
    fn withholding(self, withheld: Withheld) -> FakeIdentity {
        FakeIdentity {
            withholds: Some(withheld),
            ..self
        }
    }

    /// The token it gives after `renewed` renewals.
    fn token(&self, renewed: usize) -> String {
        match renewed {
            0 => self.token.clone(),
            n => format!("{}-{n}", self.token),
        }
    }
}

impl Identity for FakeIdentity {
    fn name(&self) -> String {
        "a fake identity".into()
    }

    fn url(&self, remote_url: &str) -> Option<String> {
        Some(self.url.clone().unwrap_or_else(|| remote_url.into()))
    }

    fn credentials(&self, url: &str) -> anyhow::Result<Credentials> {
        if let Some(own) = &self.url {
            assert_eq!(url, own);
        }
        self.asked.borrow_mut().push(url.into());
        if let Some(w) = &self.withholds {
            return Err(w.clone().into());
        }
        let n = self.renewed.get();
        Ok(Credentials {
            username: "x-fake".into(),
            password: self.token(n),
            about: format!("the fake's token number {n}"),
        })
    }

    fn renew(&self, url: &str) -> anyhow::Result<Credentials> {
        if let Some(why) = &self.refuses {
            anyhow::bail!("{why}");
        }
        self.renewed.set(self.renewed.get() + 1);
        self.credentials(url)
    }
}

/// The `Authorization` header git sends with the fake's `token`.
fn basic(token: &str) -> String {
    format!("Basic {}", base64(format!("x-fake:{token}").as_bytes()))
}

/// The `Authorization` headers the fake remote was sent, in order.
fn given(fake: &Fake) -> Vec<String> {
    fake.requests()
        .iter()
        .filter_map(|r| r.header("Authorization").map(String::from))
        .collect()
}

/// What git is answered with no credentials: a 401 asking for some.
fn ask() -> Answer {
    (
        401,
        vec![("WWW-Authenticate".into(), "Basic realm=\"fake\"".into())],
        String::new(),
    )
}

/// GitHub's answer to git with a token that may not read the repository.
fn not_found() -> Answer {
    (
        404,
        vec![("Content-Type".into(), "text/plain".into())],
        "Repository not found.\n".into(),
    )
}

/// The pushes the fake was sent, each its `Authorization` and its
/// commands, in order.
fn pushes(fake: &Fake) -> Vec<(String, Vec<(String, String, String)>)> {
    fake.requests()
        .iter()
        .filter(|r| is_push(r))
        .map(|r| (r.header("Authorization").unwrap_or("").into(), commands(r)))
        .collect()
}

/// The `Authorization` of each listing the fake gave a push, in order.
fn listed_for_push(fake: &Fake) -> Vec<String> {
    fake.requests()
        .iter()
        .filter(|r| lists_for_push(r))
        .filter_map(|r| r.header("Authorization").map(String::from))
        .collect()
}

/// A fake remote, answering each token as `answer` does for it; one that
/// gives none is asked for one.
fn remote(answer: impl Fn(&str, &Request) -> Answer + Send + Sync + 'static) -> Fake {
    Fake::start(move |r| match r.header("Authorization") {
        None => ask(),
        Some(a) => answer(a, r),
    })
}

/// The pushed refs of `repo` reached through `fake` as `identity`.
fn as_identity(repo: &Repo, identity: &Rc<FakeIdentity>) -> PushedRefs {
    PushedRefs::open_as(repo.dir(), "origin", Some(speaker(identity)), recorded()).unwrap()
}

/// `identity`, with the `sluice` git runs as its credential helper.
fn speaker(identity: &Rc<FakeIdentity>) -> (Rc<dyn Identity>, PathBuf) {
    (
        identity.clone() as Rc<dyn Identity>,
        PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
    )
}

/// The `Authorization` of each token `identity` gave, in order: the first
/// and each made afresh since.
fn tokens(identity: &FakeIdentity) -> Vec<String> {
    (0..=identity.renewed.get())
        .map(|n| basic(&identity.token(n)))
        .collect()
}

/// What a refusal that stands says of every token `identity` gave.
fn refused_note(identity: &FakeIdentity) -> String {
    let each: Vec<String> = (0..=identity.renewed.get())
        .map(|n| format!("the fake's token number {n}"))
        .collect();
    format!("origin refused {}", each.join(", then "))
}

/// A command's retry policy whose pauses are skipped, as if taken.
fn recorded() -> Rc<retry::Policy> {
    Rc::new(retry::Policy::command().recording())
}

/// [`recorded`], each try given `limit`.
fn trying(limit: Duration) -> Rc<retry::Policy> {
    Rc::new(retry::Policy::command().recording().trying_for(limit))
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// C.4: sluice's git calls get the identity's token through the helper
/// named on their own command line; nothing holds it afterwards, and git
/// run directly in a worktree gets nothing of sluice's.
#[test]
fn an_identity_reaches_only_sluices_git() {
    let repo = Repo::new("identity");
    let _bare = repo.bare_origin();
    // A remote that asks for credentials and refuses whatever it gets.
    let fake = Fake::start(|r| match r.header("Authorization") {
        Some(_) => (403, vec![], "refused\n".into()),
        None => ask(),
    });
    let token = format!("fake-token-{}", std::process::id());
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, &token));
    let p = as_identity(&repo, &identity);
    assert_eq!(p.identity().unwrap().as_deref(), Some("a fake identity"));
    assert!(p.read("main").is_err(), "the fake refuses every fetch");
    let basic = basic(&token);
    assert_eq!(
        given(&fake),
        tokens(&identity),
        "git gave the fake identity's credentials, then fresh ones"
    );

    let config = std::fs::read_to_string(repo.dir().join(".git/config")).unwrap();
    assert!(!config.contains(&token), "{config}");
    assert!(std::env::vars().all(|(_, v)| !v.contains(&token)));

    let worktree = repo.dir().join("agent");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "agent",
        worktree.to_str().unwrap(),
    ]);
    let before = fake.requests().len();
    let push = support::git_command()
        .current_dir(&worktree)
        .args(["push", &url, "HEAD:main"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!push.status.success());
    let seen = fake.requests();
    assert!(seen.len() > before, "the agent's push reached the remote");
    for r in &seen[before..] {
        let auth = r.header("Authorization").unwrap_or("");
        assert_ne!(auth, basic, "an agent's push carried sluice's token");
    }
}

/// A fetch the remote refuses the identity's credentials on is run once
/// more with credentials made afresh, and reads what the second run
/// finds.
#[test]
fn a_refused_fetch_is_run_again_with_fresh_credentials() {
    let repo = Repo::new("refused-once");
    let _bare = repo.bare_origin();
    let token = format!("fake-token-{}", std::process::id());
    let (first, fresh) = (basic(&token), basic(&format!("{token}-1")));
    let fake = Fake::start(move |r| match r.header("Authorization") {
        None => ask(),
        Some(a) if a == first => not_found(),
        Some(a) if a == fresh => empty_repository(),
        Some(_) => (403, vec![], "an unknown token\n".into()),
    });
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, &token));
    let p = as_identity(&repo, &identity);
    assert_eq!(p.read("main").unwrap(), None, "the fresh token reads");
    assert_eq!(
        given(&fake),
        [basic(&token), basic(&identity.token(1))],
        "the refused token, then a fresh one, once"
    );
}

/// deneb, 2026-11-05: a driver keeps each branch's profile open for as
/// long as it runs, and a remote pointed at its repository's new address
/// since, as after GitHub renamed it, is reached there by the next read,
/// its credentials asked for that address, with nothing opened again.
#[test]
fn a_remote_pointed_elsewhere_is_reached_there_at_the_next_read() {
    let repo = Repo::new("repointed");
    repo.write(
        ".config/sluice.toml",
        "[branches.main]\nremote = \"origin\"\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let _bare = repo.bare_origin();
    let (old, new) = (
        remote(|_, _| empty_repository()),
        remote(|_, _| empty_repository()),
    );
    let from = format!("{}/o/old.git", old.url());
    let to = format!("{}/o/new.git", new.url());
    repo.git(&["remote", "set-url", "origin", &from]);
    let identity = Rc::new(FakeIdentity::following("fake-token"));
    let b = Branches::open_as(repo.dir(), Some(speaker(&identity)), recorded()).unwrap();
    assert_eq!(b.read("main").unwrap(), None);
    let reached = old.requests().len();
    assert!(reached > 0, "the read reached the old address");

    repo.git(&["remote", "set-url", "origin", &to]);
    assert_eq!(b.read("main").unwrap(), None);
    assert_eq!(old.requests().len(), reached, "nothing more of the old");
    assert!(
        !new.requests().is_empty(),
        "the read reached the new address"
    );
    assert_eq!(*identity.asked.borrow(), [from, to]);
}

/// 133, rule core.28: a fetch the forge gives sluice's identity no
/// credential for is a read of the remote refused, in the forge's words
/// with sluice's note beside them; no git runs, and the forge's answer is
/// not asked again (rule core.29).
#[test]
fn a_credential_the_forge_withholds_is_a_refused_read() {
    let repo = Repo::new("withheld");
    let _bare = repo.bare_origin();
    let fake = remote(|_, _| empty_repository());
    let url = format!("{}/o/r.git", fake.url());
    let withheld = Withheld {
        said: "GET /repos/o/r/installation: 404 Not Found".into(),
        note: Some("point the remote that names o/r at its new address".into()),
    };
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token").withholding(withheld.clone()));
    let p = as_identity(&repo, &identity);
    let read = p.read("main").unwrap_err();
    assert_eq!(
        read_refused(&read),
        Some(&ReadRefused {
            remote: "origin".into(),
            asked: "refs/heads/main".into(),
            said: withheld.said,
            note: withheld.note,
        })
    );
    assert!(fake.requests().is_empty(), "no git ran");
    assert_eq!(identity.asked.borrow().len(), 1, "asked once");
}

/// 134: a fetch refused on fresh credentials too, until the retry
/// policy's time is spent, fails with the remote's words and what each
/// refused credential was, in the order tried.
#[test]
fn a_refusal_that_stands_names_every_credential() {
    let repo = Repo::new("refused-twice");
    let _bare = repo.bare_origin();
    let fake = Fake::start(|r| match r.header("Authorization") {
        None => ask(),
        Some(_) => not_found(),
    });
    let token = format!("fake-token-{}", std::process::id());
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, &token));
    let p = as_identity(&repo, &identity);
    let read = p.read("main").unwrap_err();
    let err = format!("{read:#}");
    assert!(err.contains("Repository not found."), "{err}");
    let note = refused_note(&identity);
    assert!(err.contains(&note), "{err}");
    assert!(!err.contains(&token), "a report carries no token: {err}");
    assert_eq!(given(&fake), tokens(&identity), "a fresh token each try");
    assert!(identity.renewed.get() >= 2, "more than two tries");
    // 133: the note is sluice's, apart from what git said.
    let refused = read_refused(&read).expect("a refused read");
    assert_eq!(refused.asked, "refs/heads/main");
    assert!(
        refused.said.contains("Repository not found."),
        "{refused:?}"
    );
    assert!(!refused.said.contains("refused the fake's"), "{refused:?}");
    assert_eq!(refused.note.as_deref(), Some(note.as_str()));
}

/// 134, C.5: a session's branch the remote refuses on the identity's first
/// token, as GitHub refuses one it has just given, is pushed once more on
/// a fresh one.
#[test]
fn a_push_refused_on_its_token_is_made_once_more() {
    let repo = Repo::new("push-renewed");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = repo.branch("unit/u", "main", &[("a", Some("1"))]);
    let first = basic("fake-token");
    let fake = {
        let first = first.clone();
        remote(move |a, r| match a == first {
            true => not_found(),
            false => takes(r, &[("refs/heads/main", &base)]),
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    p.push_branch(&rev(head.clone()), "unit/u").unwrap();
    let fresh = basic(&identity.token(1));
    assert_eq!(given(&fake), [first, fresh.clone(), fresh.clone()]);
    let zero = "0".repeat(40);
    assert_eq!(
        pushes(&fake),
        [(fresh, vec![(zero, head, "refs/heads/unit/u".into())])]
    );
    assert_eq!(identity.renewed.get(), 1);
}

/// 134: a push refused on every token until the retry policy's time is
/// spent fails with the remote's words and what each refused token was,
/// in the order tried, and carries no token.
#[test]
fn a_push_refused_on_every_token_names_each_in_order() {
    let repo = Repo::new("refused-push");
    let _bare = repo.bare_origin();
    let fake = remote(|_, _| not_found());
    let token = format!("fake-token-{}", std::process::id());
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, &token));
    let p = as_identity(&repo, &identity);
    let head = rev(repo.rev("HEAD"));
    let err = format!("{:#}", p.push_branch(&head, "agent").unwrap_err());
    assert!(err.contains("Repository not found."), "{err}");
    assert!(err.ends_with(&refused_note(&identity)), "{err}");
    assert!(!err.contains(&token), "a report carries no token: {err}");
    assert_eq!(given(&fake), tokens(&identity), "a fresh token each try");
    assert!(identity.renewed.get() >= 2, "more than two tries");
}

/// 134 (forge-identity, refused twice): a push refused on its token and on
/// a fresh one, the policy's time spent before another try, is two pushes
/// whose failure names both tokens.
#[test]
fn a_push_refused_twice_with_the_time_spent_names_both() {
    let repo = Repo::new("refused-push-twice");
    let _bare = repo.bare_origin();
    let fake = remote(|_, _| not_found());
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    // Room for one pause of a second or two, and no second pause.
    let policy = Rc::new(retry::Policy::new(Duration::from_secs(2)).recording());
    let p = PushedRefs::open_as(repo.dir(), "origin", Some(speaker(&identity)), policy).unwrap();
    let head = rev(repo.rev("HEAD"));
    let err = format!("{:#}", p.push_branch(&head, "agent").unwrap_err());
    assert!(err.contains("Repository not found."), "{err}");
    assert!(
        err.ends_with("origin refused the fake's token number 0, then the fake's token number 1"),
        "{err}"
    );
    assert_eq!(given(&fake).len(), 2, "two pushes");
}

/// 134: a push refused on its token, when its forge makes no fresh one,
/// fails with the remote's words, what the refused token was and why none
/// could be made afresh, after one push.
#[test]
fn a_push_with_no_fresh_token_is_made_once() {
    let repo = Repo::new("push-unrenewed");
    let _bare = repo.bare_origin();
    let fake = remote(|_, _| not_found());
    let token = format!("fake-token-{}", std::process::id());
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, &token).refusing("the forge made no token"));
    let p = as_identity(&repo, &identity);
    let head = rev(repo.rev("HEAD"));
    let err = format!("{:#}", p.push_branch(&head, "agent").unwrap_err());
    assert!(err.contains("Repository not found."), "{err}");
    assert!(
        err.ends_with(
            "origin refused the fake's token number 0, \
             and none could be made afresh: the forge made no token"
        ),
        "{err}"
    );
    assert!(!err.contains(&token), "a report carries no token: {err}");
    assert_eq!(given(&fake).len(), 1, "one push");
    assert_eq!(identity.renewed.get(), 0);
}

/// 134, 45, 104, rule core.5: a leased push the remote refuses on the
/// identity's first token is made once more on a fresh one with the same
/// lease, and moves the branch; a branch made as a landing is.
#[test]
fn a_leased_push_refused_on_its_token_is_made_once_more() {
    let repo = Repo::new("leased-renewed");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let local = open(&repo);
    let to = landing(&local, &rev(base.clone()), &head);
    let zero = "0".repeat(40);
    for (branch, old, new) in [
        ("bolt/x", zero, base.clone()),
        ("main", base.clone(), to.as_str().to_string()),
    ] {
        let first = basic("fake-token");
        let fake = {
            let (first, base) = (first.clone(), base.clone());
            remote(move |a, r| match is_push(r) && a == first {
                true => not_found(),
                false => takes(r, &[("refs/heads/main", &base)]),
            })
        };
        let url = format!("{}/o/r.git", fake.url());
        let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
        let p = as_identity(&repo, &identity);
        let moved = match branch {
            "main" => p.move_to("main", &rev(base.clone()), &to),
            _ => p.make(branch, &rev(base.clone())),
        };
        assert_eq!(moved.unwrap(), Moved::Moved, "{branch}");
        let command = vec![(old, new, format!("refs/heads/{branch}"))];
        assert_eq!(
            pushes(&fake),
            [
                (first, command.clone()),
                (basic(&identity.token(1)), command)
            ],
            "{branch}: the refused push, then the same on a fresh token"
        );
        assert_eq!(identity.renewed.get(), 1);
    }
}

/// 134, 46, rule core.5: a branch moved on the remote between a push
/// refused on its token and the second is the second's lease refused: the
/// move is elsewhere, and there is no third push.
#[test]
fn a_second_push_carries_the_first_ones_lease() {
    let repo = Repo::new("leased-stale");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let moved = repo.branch("other", "main", &[("b", Some("2"))]);
    let local = open(&repo);
    let to = landing(&local, &rev(base.clone()), &head);
    let first = basic("fake-token");
    let fake = {
        let (first, base, moved) = (first.clone(), base.clone(), moved.clone());
        remote(move |a, r| match (a == first, is_push(r)) {
            (true, true) => not_found(),
            (true, false) => takes(r, &[("refs/heads/main", &base)]),
            (false, _) => takes(r, &[("refs/heads/main", &moved)]),
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(
        p.move_to("main", &rev(base.clone()), &to).unwrap(),
        Moved::Elsewhere(Some(rev(moved)))
    );
    assert_eq!(
        pushes(&fake),
        [(
            first.clone(),
            vec![(base, to.as_str().into(), "refs/heads/main".into())]
        )],
        "the second push went no further than the lease"
    );
    assert_eq!(
        listed_for_push(&fake),
        [first, basic(&identity.token(1))],
        "a listing for each push, and no third"
    );
    assert_eq!(identity.renewed.get(), 1);
}

/// 134, 24, 45: a push the remote answered at the ref is the move refused
/// and made once, whatever the remote said besides; so is one whose lease
/// git found stale.
#[test]
fn a_push_refused_at_the_ref_is_made_once() {
    let repo = Repo::new("refused-at-the-ref");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let moved = repo.branch("other", "main", &[("b", Some("2"))]);
    let local = open(&repo);
    let to = landing(&local, &rev(base.clone()), &head);

    let fake = {
        let base = base.clone();
        remote(move |_, r| match is_push(r) {
            true => report(
                &["ng refs/heads/main protected branch hook declined".into()],
                Some("error: 403 Forbidden: refs/heads/main is protected"),
            ),
            false => takes(r, &[("refs/heads/main", &base)]),
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    let err = format!(
        "{:#}",
        p.move_to("main", &rev(base.clone()), &to).unwrap_err()
    );
    assert!(err.contains("protected branch hook declined"), "{err}");
    assert!(err.contains("error: 403 Forbidden"), "{err}");
    assert_eq!(pushes(&fake).len(), 1, "one push: {err}");
    assert_eq!(identity.renewed.get(), 0);

    let fake = {
        let moved = moved.clone();
        remote(move |_, r| takes(r, &[("refs/heads/main", &moved)]))
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(
        p.move_to("main", &rev(base), &to).unwrap(),
        Moved::Elsewhere(Some(rev(moved)))
    );
    assert_eq!(listed_for_push(&fake), [basic("fake-token")], "one push");
    assert!(pushes(&fake).is_empty(), "git found the lease stale");
    assert_eq!(identity.renewed.get(), 0);
}

/// sluice runs unattended: a push to a remote that wants a credential the
/// host cannot give fails at once with an error, never a prompt or a wait.
#[test]
fn a_push_with_no_credential_fails_rather_than_waits() {
    let repo = Repo::new("no-credential");
    let fake = Fake::start(|_| {
        (
            401,
            vec![("WWW-Authenticate".into(), "Basic realm=\"fake\"".into())],
            String::new(),
        )
    });
    repo.git(&[
        "remote",
        "add",
        "origin",
        &format!("{}/o/r.git", fake.url()),
    ]);
    let head = rev(repo.rev("main"));
    let dir = repo.dir().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let p = PushedRefs::open_as(&dir, "origin", None, recorded()).unwrap();
        let read = p.read("main").map_err(|e| e.to_string());
        let push = p.move_to("main", &head, &head).map_err(|e| e.to_string());
        let _ = tx.send((read, push));
    });
    let (read, push) = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("git waited instead of failing");
    let read = read.unwrap_err();
    assert!(read.contains("fetching main from origin failed"), "{read}");
    let push = push.unwrap_err();
    assert!(push.contains("pushing main to origin failed"), "{push}");
}

/// C.5: a session's branch pushed through a remote that takes the
/// identity's token is one push, naming the branch and the revision.
#[test]
fn a_push_the_remote_takes_is_one_push() {
    let repo = Repo::new("push-taken");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = repo.branch("unit/u", "main", &[("a", Some("1"))]);
    let fake = {
        let base = base.clone();
        remote(move |_, r| takes(r, &[("refs/heads/main", &base)]))
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    p.push_branch(&rev(head.clone()), "unit/u").unwrap();
    let zero = "0".repeat(40);
    assert_eq!(
        pushes(&fake),
        [(
            basic("fake-token"),
            vec![(zero, head, "refs/heads/unit/u".into())]
        )]
    );
    assert_eq!(identity.renewed.get(), 0);
}

/// 134, 122, 104: a branch made whose push git read no answer to, the
/// remote failing on it, is not pushed again; what the remote holds then
/// decides: made when it holds the branch at the revision pushed, failed
/// naming its absence otherwise.
#[test]
fn a_branch_made_with_no_answer_is_what_the_remote_holds() {
    let repo = Repo::new("made-unanswered");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let at = repo.branch("unit/u", "main", &[("a", Some("1"))]);
    for made in [true, false] {
        let fake = {
            let (base, at) = (base.clone(), at.clone());
            remote(move |_, r| {
                if lists_for_push(r) {
                    listing("git-receive-pack", &[("refs/heads/main", &base)])
                } else if is_push(r) {
                    (500, vec![], "the server failed\n".into())
                } else if made {
                    let refs = [
                        ("refs/heads/bolt/x", at.as_str()),
                        ("refs/heads/main", &base),
                    ];
                    listing("git-upload-pack", &refs)
                } else {
                    listing("git-upload-pack", &[("refs/heads/main", &base)])
                }
            })
        };
        let url = format!("{}/o/r.git", fake.url());
        let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
        let p = as_identity(&repo, &identity);
        let moved = p.make("bolt/x", &rev(at.clone()));
        if made {
            assert_eq!(moved.unwrap(), Moved::Moved);
        } else {
            let err = format!("{:#}", moved.unwrap_err());
            assert!(err.contains("pushing bolt/x to origin failed"), "{err}");
            assert!(
                err.ends_with("the push went unanswered, and origin has no bolt/x"),
                "{err}"
            );
        }
        assert_eq!(pushes(&fake).len(), 1, "made: {made}, one push");
        assert_eq!(identity.renewed.get(), 0);
    }
}

/// 134, 122, 105: a branch removed whose push's answer was lost, the
/// connection cut, and which the remote no longer holds is removed: its
/// local branch and remote-tracking ref go as an answered removal's do.
#[test]
fn a_branch_removed_with_no_answer_is_what_the_remote_holds() {
    let repo = Repo::new("removed-unanswered");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let at = repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    repo.git(&["update-ref", "refs/remotes/origin/bolt/x", &at]);
    let fake = {
        let (base, at) = (base.clone(), at.clone());
        remote(move |_, r| {
            if lists_for_push(r) {
                let refs = [
                    ("refs/heads/bolt/x", at.as_str()),
                    ("refs/heads/main", &base),
                ];
                listing("git-receive-pack", &refs)
            } else if is_push(r) {
                (CUT, vec![], String::new())
            } else {
                listing("git-upload-pack", &[("refs/heads/main", &base)])
            }
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(p.remove("bolt/x", &rev(at.clone())).unwrap(), Moved::Moved);
    let zero = "0".repeat(40);
    assert_eq!(
        pushes(&fake),
        [(
            basic("fake-token"),
            vec![(at, zero, "refs/heads/bolt/x".into())]
        )],
        "one push"
    );
    for gone in ["refs/heads/bolt/x", "refs/remotes/origin/bolt/x"] {
        let out = repo.git_raw(&["rev-parse", "--verify", "-q", gone]);
        assert!(!out.status.success(), "{gone} is left");
    }
}

/// 134: a fetch whose request for the remote's refs is answered 503 is
/// tried again on the same token, and reads once the remote answers.
#[test]
fn a_fetch_answered_503_twice_reads_on_the_third() {
    let repo = Repo::new("fetch-503");
    let _bare = repo.bare_origin();
    let fetches = Count::default();
    let fake = {
        let fetches = fetches.clone();
        remote(move |_, _| match fetches.take() {
            0 | 1 => unavailable(),
            _ => empty_repository(),
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(p.read("main").unwrap(), None, "the third fetch reads");
    assert_eq!(given(&fake), vec![basic("fake-token"); 3]);
    assert_eq!(identity.renewed.get(), 0, "a 503 buys no token");
}

/// 134, 45, rule core.5: a leased push refused `Repository not found.` on
/// two tokens and taken on the third moves the branch: three pushes, each
/// on a different token, with the same lease and ref.
#[test]
fn a_leased_push_refused_on_two_tokens_lands_on_the_third() {
    let repo = Repo::new("leased-third");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let to = landing(&open(&repo), &rev(base.clone()), &head);
    let refused = [basic("fake-token"), basic("fake-token-1")];
    let fake = {
        let base = base.clone();
        remote(
            move |a, r| match is_push(r) && refused.iter().any(|t| t == a) {
                true => not_found(),
                false => takes(r, &[("refs/heads/main", &base)]),
            },
        )
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(
        p.move_to("main", &rev(base.clone()), &to).unwrap(),
        Moved::Moved
    );
    let command = vec![(base, to.as_str().to_string(), "refs/heads/main".into())];
    assert_eq!(
        pushes(&fake),
        tokens(&identity)
            .into_iter()
            .map(|t| (t, command.clone()))
            .collect::<Vec<_>>(),
        "three pushes, each on its own token, each the same"
    );
    assert_eq!(identity.renewed.get(), 2);
}

/// 134 (forge-identity): a push whose request for the remote's refs is
/// answered 503, before anything of it was sent, is tried again and lands.
#[test]
fn a_push_answered_503_before_it_was_sent_is_tried_again() {
    let repo = Repo::new("push-503");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let to = landing(&open(&repo), &rev(base.clone()), &head);
    let listed = Count::default();
    let fake = {
        let (base, listed) = (base.clone(), listed.clone());
        remote(move |_, r| match lists_for_push(r) && listed.take() == 0 {
            true => unavailable(),
            false => takes(r, &[("refs/heads/main", &base)]),
        })
    };
    let url = format!("{}/o/r.git", fake.url());
    let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
    let p = as_identity(&repo, &identity);
    assert_eq!(p.move_to("main", &rev(base), &to).unwrap(), Moved::Moved);
    assert_eq!(listed_for_push(&fake), vec![basic("fake-token"); 2]);
    assert_eq!(pushes(&fake).len(), 1, "the first try sent nothing");
}

/// 134, 122, 45: a landing whose push was cut after it was sent is made
/// once; the remote's branch then decides: moved when it holds the
/// landing, failed naming the unanswered push and where the branch is
/// otherwise.
#[test]
fn a_landing_cut_after_it_was_sent_is_what_the_remote_holds() {
    let repo = Repo::new("landing-cut");
    let _bare = repo.bare_origin();
    let base = repo.rev("main");
    let head = rev(repo.branch("unit/u", "main", &[("a", Some("1"))]));
    let to = landing(&open(&repo), &rev(base.clone()), &head);
    for made in [true, false] {
        let pushed = Count::default();
        let fake = {
            let (base, to, pushed) = (base.clone(), to.as_str().to_string(), pushed.clone());
            remote(move |_, r| {
                if lists_for_push(r) {
                    listing("git-receive-pack", &[("refs/heads/main", &base)])
                } else if is_push(r) {
                    pushed.take();
                    (CUT, vec![], String::new())
                } else if made && pushed.get() > 0 {
                    listing("git-upload-pack", &[("refs/heads/main", &to)])
                } else {
                    listing("git-upload-pack", &[("refs/heads/main", &base)])
                }
            })
        };
        let url = format!("{}/o/r.git", fake.url());
        let identity = Rc::new(FakeIdentity::new(&url, "fake-token"));
        let p = as_identity(&repo, &identity);
        let moved = p.move_to("main", &rev(base.clone()), &to);
        if made {
            assert_eq!(moved.unwrap(), Moved::Moved);
        } else {
            let err = format!("{:#}", moved.unwrap_err());
            assert!(err.contains("pushing main to origin failed"), "{err}");
            assert!(
                err.ends_with(&format!(
                    "the push went unanswered, and origin holds main at {base}"
                )),
                "{err}"
            );
        }
        assert_eq!(pushes(&fake).len(), 1, "made: {made}, one push");
    }
}

/// 134: on the host's own credentials, which sluice cannot make afresh, a
/// push the remote refuses is made once and fails with the remote's words.
#[test]
fn a_push_refused_on_the_hosts_credentials_is_made_once() {
    let repo = Repo::new("host-refused");
    let fake = Fake::start(|_| not_found());
    repo.git(&[
        "remote",
        "add",
        "origin",
        &format!("{}/o/r.git", fake.url()),
    ]);
    let p = open(&repo);
    let head = rev(repo.rev("main"));
    let err = format!("{:#}", p.push_branch(&head, "agent").unwrap_err());
    assert!(err.contains("Repository not found."), "{err}");
    assert!(!err.contains("refused"), "no token to name: {err}");
    let tried = fake
        .requests()
        .iter()
        .filter(|r| r.path.contains("git-receive-pack"))
        .count();
    assert_eq!(tried, 1, "one push");
}

/// 129: the remotes and a remote's fetch address, read with no process,
/// are what `git remote` and `git config --get remote.<name>.url`
/// answer, an `insteadOf` left unapplied.
#[test]
fn the_remotes_and_their_addresses_read_as_git_answers_them() {
    let repo = Repo::new("remotes-read");
    let bare = repo.bare_origin();
    repo.git(&["remote", "add", "forge", "git@github.com:acme/widgets.git"]);
    repo.git(&["remote", "add", "web", "https://github.com/acme/widgets"]);
    repo.git(&[
        "config",
        "url.https://github.com/.insteadOf",
        "git@github.com:",
    ]);
    let local = sluice::profiles::local_refs::LocalRefs::open(repo.dir()).unwrap();
    let listed: Vec<String> = repo.git(&["remote"]).lines().map(str::to_string).collect();
    assert_eq!(listed, ["forge", "origin", "web"]);
    assert_eq!(local.remotes().unwrap(), listed);
    for r in &listed {
        let configured = repo.git(&["config", "--get", &format!("remote.{r}.url")]);
        assert_eq!(local.remote_url(r).unwrap(), configured, "{r}");
    }
    assert_eq!(
        sluice::profiles::pushed_refs::remote_url(repo.dir(), "origin").unwrap(),
        bare.path().to_str().unwrap()
    );
    assert!(local.remote_url("nowhere").is_err());
}

/// 129, rule core.27: a remote added by `git remote add` after the
/// profile opened is read, and routes the branch whose policy its
/// tracking ref holds, at the next read.
#[test]
fn a_remote_added_after_the_profile_opened_routes_at_its_next_read() {
    let up = Repo::new("added-up");
    up.write(".config/sluice.toml", "[branches.main]\nremote = \"up\"\n");
    up.git(&["add", "-A"]);
    up.git(&["commit", "-q", "-m", "policy"]);
    let bare = up.bare_origin();
    let repo = Repo::new("added");
    let b = Branches::open_as(repo.dir(), None, recorded()).unwrap();
    assert_eq!(b.remote("main").unwrap(), None);
    repo.git(&["remote", "add", "up", bare.path().to_str().unwrap()]);
    repo.git(&["fetch", "-q", "up"]);
    assert_eq!(b.remotes().unwrap(), ["up"]);
    assert_eq!(b.remote("main").unwrap().as_deref(), Some("up"));
    assert_eq!(b.read("main").unwrap(), Some(rev(up.rev("main"))));
}

/// 129: a branch held as last fetched is read with the remote out of
/// reach, at what was last fetched, though the remote has moved since:
/// nothing is fetched, where a read is refused.
#[test]
fn a_branch_held_fetches_nothing_with_the_remote_out_of_reach() {
    let repo = Repo::new("held");
    let bare = repo.bare_origin();
    let p = open(&repo);
    let fetched = p.read("main").unwrap();
    assert_eq!(fetched, Some(rev(repo.rev("main"))));
    Repo::clone_of(bare.path(), "held-other").push_change("b", "2\n");
    let away = bare.path().with_extension("away");
    std::fs::rename(bare.path(), &away).unwrap();
    let held = p.held("main");
    let read = p.read("main");
    std::fs::rename(&away, bare.path()).unwrap();
    assert_eq!(held.unwrap(), fetched);
    assert!(read_refused(&read.unwrap_err()).is_some());
    assert_eq!(p.held("none").unwrap(), None);
}
