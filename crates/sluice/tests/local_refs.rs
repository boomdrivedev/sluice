//! The `local refs` profile against real repositories: section 1's
//! acceptance tests in `docs/plans/phase-1-integrations.md`. The
//! check-tools tier: run with `--features tools`.

#![cfg(feature = "tools")]

mod support;

use sluice::identity::Author;
use sluice::profiles::local_refs::LocalRefs;
use sluice_seams::{Branch, CoveringVerdict, Formed, HostCopy, LandingCommit, Moved, Rev};
use support::Repo;

fn rev(s: String) -> Rev {
    Rev::new(s)
}

fn refs(repo: &Repo) -> LocalRefs {
    LocalRefs::open(repo.dir()).expect("open")
}

fn formed(f: Formed) -> Rev {
    match f {
        Formed::Revision(r) => r,
        Formed::Conflict { paths } => panic!("unexpected conflict on {paths:?}"),
    }
}

fn conflict(f: Formed) -> Vec<String> {
    match f {
        Formed::Conflict { paths } => paths,
        Formed::Revision(r) => panic!("expected a conflict, formed {r}"),
    }
}

/// 1, 42: an existing, a missing and a packed branch.
#[test]
fn read_existing_missing_and_packed() {
    let repo = Repo::new("read");
    let head = repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    let r = refs(&repo);
    assert_eq!(r.read("bolt/x").unwrap(), Some(rev(head.clone())));
    assert_eq!(r.read("bolt/none").unwrap(), None);
    repo.git(&["pack-refs", "--all"]);
    assert!(!repo.dir().join(".git/refs/heads/bolt/x").exists());
    assert_eq!(refs(&repo).read("bolt/x").unwrap(), Some(rev(head)));
}

/// Rule core.66 (design D3): a repository is named by the `origin` it
/// configures, its owner and name and nothing else of the URL, and with no
/// origin by its folder; read with no fetch, so an origin no host answers
/// for names it all the same.
#[test]
fn a_repository_is_named_by_the_origin_it_configures() {
    let repo = Repo::new("app");
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://user:token@git.example/acme/app.git",
    ]);
    assert_eq!(refs(&repo).repository_name().unwrap(), "acme/app");
    assert!(!repo.dir().join(".git/FETCH_HEAD").exists());

    let alone = Repo::new("alone");
    let folder = alone.dir().file_name().unwrap().to_str().unwrap();
    assert_eq!(refs(&alone).repository_name().unwrap(), folder);
}

/// 124: a branch in this host's refs is its own copy, and a levelling of
/// it is refused, moving nothing.
#[test]
fn a_branch_in_this_hosts_refs_has_no_copy_apart() {
    let repo = Repo::new("itself");
    let head = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let base = rev(repo.rev("main"));
    let r = refs(&repo);
    assert_eq!(r.copy("bolt/x").unwrap(), HostCopy::Itself);
    assert!(r.level("bolt/x", Some(&head), &base).is_err());
    assert_eq!(repo.rev("bolt/x"), head.as_str());
}

/// 2, 43: a clean candidate is git's merge, a commit of the two.
#[test]
fn form_clean_is_gits_merge_with_both_parents() {
    let repo = Repo::new("clean");
    let onto = repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    let head = repo.branch("unit/u", "main", &[("b", Some("2"))]);
    let c = formed(
        refs(&repo)
            .form(&rev(onto.clone()), &rev(head.clone()))
            .unwrap(),
    );
    let tree = repo.git(&["merge-tree", "--write-tree", &onto, &head]);
    assert_eq!(repo.rev(&format!("{c}^{{tree}}")), tree);
    assert_eq!(
        repo.git(&["rev-parse", &format!("{c}^1"), &format!("{c}^2")]),
        format!("{onto}\n{head}")
    );
    // One combination forms one commit, whenever it is formed.
    let again = formed(refs(&repo).form(&rev(onto), &rev(head)).unwrap());
    assert_eq!(c, again);
}

/// 3, 12, 65: content, modify/delete, rename/rename and add/add.
#[test]
fn form_conflicts_name_every_path() {
    let repo = Repo::new("conflicts");
    repo.git(&["switch", "-q", "main"]);
    repo.write("plan.py", "base\n");
    repo.write("del.txt", "keep\n");
    repo.write("old.txt", "rename me\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "base files"]);
    repo.git(&["switch", "-q", "--detach", "main"]);
    let r = refs(&repo);

    let a = repo.branch("unit/a", "main", &[("plan.py", Some("a\n"))]);
    let b = repo.branch("unit/b", "main", &[("plan.py", Some("b\n"))]);
    assert_eq!(conflict(r.form(&rev(a), &rev(b)).unwrap()), vec!["plan.py"]);

    let del = repo.branch("unit/del", "main", &[("del.txt", None)]);
    let modi = repo.branch("unit/mod", "main", &[("del.txt", Some("changed\n"))]);
    assert_eq!(
        conflict(r.form(&rev(del), &rev(modi)).unwrap()),
        vec!["del.txt"]
    );

    let r1 = repo.branch_with("unit/r1", "main", &[], &[("old.txt", "new.txt")]);
    let r2 = repo.branch_with("unit/r2", "main", &[], &[("old.txt", "other-name.txt")]);
    let mut paths = conflict(r.form(&rev(r1), &rev(r2)).unwrap());
    paths.sort();
    assert_eq!(paths, vec!["new.txt", "old.txt", "other-name.txt"]);

    let x = repo.branch("unit/x", "main", &[("added.txt", Some("x\n"))]);
    let y = repo.branch("unit/y", "main", &[("added.txt", Some("y\n"))]);
    assert_eq!(
        conflict(r.form(&rev(x), &rev(y)).unwrap()),
        vec!["added.txt"]
    );
}

/// 4, 10: a chain of three, the third conflicting only with the first.
#[test]
fn form_a_chain_of_three() {
    let repo = Repo::new("chain");
    let r = refs(&repo);
    let base = rev(repo.rev("main"));
    let one = repo.branch("unit/1", "main", &[("shared", Some("one\n"))]);
    let two = repo.branch("unit/2", "main", &[("other", Some("two\n"))]);
    let three = repo.branch("unit/3", "main", &[("shared", Some("three\n"))]);
    let c1 = formed(r.form(&base, &rev(one)).unwrap());
    let c2 = formed(r.form(&c1, &rev(two)).unwrap());
    assert_eq!(
        conflict(r.form(&c2, &rev(three.clone())).unwrap()),
        vec!["shared"]
    );
    // Alone on the branch it is clean: the conflict is with the first.
    formed(r.form(&base, &rev(three)).unwrap());
}

/// 5: a missing object and unrelated histories are errors, not conflicts.
#[test]
fn form_errors_are_not_conflicts() {
    let repo = Repo::new("errors");
    let r = refs(&repo);
    let main = rev(repo.rev("main"));
    let missing = rev("0123456789abcdef0123456789abcdef01234567".into());
    assert!(r.form(&main, &missing).is_err());
    repo.git(&["switch", "-q", "--orphan", "orphan"]);
    repo.write("lonely", "1\n");
    repo.git(&["add", "lonely"]);
    repo.git(&["commit", "-q", "-m", "orphan"]);
    let orphan = rev(repo.rev("HEAD"));
    assert!(r.form(&main, &orphan).is_err());
}

/// 6, 23, 44: one landing commit, its tree the tested candidate's.
#[test]
fn commit_writes_the_landing() {
    let repo = Repo::new("commit");
    let r = refs(&repo);
    let onto = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let head = rev(repo.branch("unit/u", "main", &[("b", Some("2"))]));
    let candidate = formed(r.form(&onto, &head).unwrap());
    let message = "Land #12: unit/u\n\nSluice-Submission: 12\nSluice-From: agent (unit/u)\nSluice-Tested: CAND\nSluice-Result: passed (2 run, 0 replayed)\n"
        .replace("CAND", candidate.as_str());
    let landed = r
        .commit(
            "bolt/x",
            &LandingCommit {
                tree_of: candidate.clone(),
                parent: onto.clone(),
                head: head.clone(),
                message,
            },
        )
        .unwrap();
    let raw = repo.git(&["cat-file", "-p", landed.as_str()]);
    assert!(
        raw.contains(&format!("parent {onto}\nparent {head}\n")),
        "{raw}"
    );
    assert!(raw.contains("\nauthor sluice <sluice@localhost> "), "{raw}");
    assert!(
        raw.contains("\ncommitter sluice <sluice@localhost> "),
        "{raw}"
    );
    assert_eq!(
        repo.rev(&format!("{landed}^{{tree}}")),
        repo.rev(&format!("{candidate}^{{tree}}"))
    );
    let body = repo.git(&["log", "-1", "--format=%B", landed.as_str()]);
    let out = support::git_command()
        .args(["interpret-trailers", "--parse"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(body.as_bytes())?;
            c.wait_with_output()
        })
        .unwrap();
    let trailers = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        trailers
            .lines()
            .filter(|l| l.starts_with("Sluice-"))
            .count(),
        4,
        "{trailers}"
    );
}

/// Record 0019 as amended 2026-11-18; rule core.43: a landing written as
/// an author names it as its author and committer, its tree the
/// combination's, while the combination stays sluice's.
#[test]
fn commit_as_names_its_author() {
    let repo = Repo::new("commit-as");
    let r = refs(&repo);
    let onto = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let head = rev(repo.branch("unit/u", "main", &[("b", Some("2"))]));
    let candidate = formed(r.form(&onto, &head).unwrap());
    let author = Author {
        name: "a fake".into(),
        email: "fake@identity.invalid".into(),
    };
    let landed = r
        .commit_as(
            &LandingCommit {
                tree_of: candidate.clone(),
                parent: onto.clone(),
                head: head.clone(),
                message: "Land #12: unit/u\n".into(),
            },
            &author,
        )
        .unwrap();
    let raw = repo.git(&["cat-file", "-p", landed.as_str()]);
    assert!(
        raw.contains("\nauthor a fake <fake@identity.invalid> "),
        "{raw}"
    );
    assert!(
        raw.contains("\ncommitter a fake <fake@identity.invalid> "),
        "{raw}"
    );
    assert!(
        raw.contains(&format!("parent {onto}\nparent {head}\n")),
        "{raw}"
    );
    assert_eq!(
        repo.rev(&format!("{landed}^{{tree}}")),
        repo.rev(&format!("{candidate}^{{tree}}"))
    );
    let formed_raw = repo.git(&["cat-file", "-p", candidate.as_str()]);
    assert!(
        formed_raw.contains("\nauthor sluice <sluice@localhost> 0 +0000\n"),
        "{formed_raw}"
    );
}

/// 23, 140; record 0083; rule core.39: a landing beside committed as
/// sluice writes it reads to git as passed: its `Sluice-Result` is one
/// line whose first word is `passed`, whole as written, by either reading
/// of the trailer, and it has no `Sluice-Tested`.
#[test]
fn a_landing_beside_reads_as_passed_to_git() {
    let repo = Repo::new("beside");
    let r = refs(&repo);
    let onto = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let head = rev(repo.branch("unit/d", "main", &[("docs/guide.md", Some("guide"))]));
    let candidate = formed(r.form(&onto, &head).unwrap());
    let sub: sluice_core::records::Submission = serde_json::from_value(serde_json::json!({
        "schema": sluice_core::records::SUBMISSION_SCHEMA, "id": 13, "branch": "bolt/x",
        "head": head.as_str(), "from": { "agent": "agent", "worktree": "unit/d" },
        "submitted": "2026-11-05T10:00:00Z", "after": [], "priority": 0,
        "attempt": 1, "state": "landing",
    }))
    .unwrap();
    let covering = CoveringVerdict {
        at: onto.clone(),
        result: "passed".into(),
        ran: 141,
        replayed: 0,
    };
    let message = sluice_core::text::beside_message(
        sluice_core::policy::LANDING_SUBJECT,
        "bolt/x",
        &sub,
        &covering,
    );
    let landed = r
        .commit(
            "bolt/x",
            &LandingCommit {
                tree_of: candidate,
                parent: onto.clone(),
                head,
                message,
            },
        )
        .unwrap();
    let written = format!(
        "passed (0 run, 0 replayed) beside the queue: no required check reads its changes; \
         covered by the result at {onto}, passed (141 run, 0 replayed)"
    );
    assert_eq!(onto.as_str().len(), 40);
    for format in [
        "--format=%(trailers:key=Sluice-Result,valueonly)",
        "--format=%(trailers:key=Sluice-Result,valueonly,separator=%x20)",
    ] {
        let result = repo.git(&["log", "-1", format, landed.as_str()]);
        assert_eq!(result.lines().count(), 1, "{format}: {result}");
        assert_eq!(result.split_whitespace().next(), Some("passed"), "{format}");
        assert_eq!(result, written, "{format}");
    }
    let tested = repo.git(&[
        "log",
        "-1",
        "--format=%(trailers:key=Sluice-Tested,valueonly)",
        landed.as_str(),
    ]);
    assert_eq!(tested, "");
}

/// 7, S8, 24, 45: moved from the expected revision, refused from a stale
/// one, a deleted branch reported, a held lock an error.
#[test]
fn move_to_from_expected_only() {
    let repo = Repo::new("move");
    let r = refs(&repo);
    let old = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let new = rev(repo.branch("unit/u", "bolt/x", &[("b", Some("2"))]));
    assert_eq!(r.move_to("bolt/x", &old, &new).unwrap(), Moved::Moved);
    assert_eq!(r.read("bolt/x").unwrap(), Some(new.clone()));
    assert!(repo
        .git(&["reflog", "bolt/x"])
        .contains("sluice: land on bolt/x"));

    // Stale: found elsewhere, reported, the branch left where it is.
    assert_eq!(
        r.move_to("bolt/x", &old, &old).unwrap(),
        Moved::Elsewhere(Some(new.clone()))
    );
    assert_eq!(r.read("bolt/x").unwrap(), Some(new.clone()));

    // Held by git's lock protocol: an error, tried again by a later step.
    let lock = repo.dir().join(".git/refs/heads/bolt/x.lock");
    std::fs::write(&lock, "").unwrap();
    assert!(r.move_to("bolt/x", &new, &old).is_err());
    std::fs::remove_file(&lock).unwrap();

    repo.git(&["branch", "-D", "bolt/x"]);
    assert_eq!(
        r.move_to("bolt/x", &new, &old).unwrap(),
        Moved::Elsewhere(None)
    );
    assert_eq!(r.read("bolt/x").unwrap(), None);
}

/// 8, 46: two moves from one expected revision; exactly one succeeds.
#[test]
fn two_moves_from_one_revision_one_wins() {
    let repo = Repo::new("race");
    let old = repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    let a = repo.branch("unit/a", "bolt/x", &[("b", Some("2"))]);
    let b = repo.branch("unit/b", "bolt/x", &[("c", Some("3"))]);
    let dir = repo.dir().to_path_buf();
    let handles: Vec<_> = [a, b]
        .into_iter()
        .map(|to| {
            let (dir, old) = (dir.clone(), old.clone());
            std::thread::spawn(move || {
                let r = LocalRefs::open(&dir).unwrap();
                r.move_to("bolt/x", &Rev::new(old), &Rev::new(to))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let moved = results
        .iter()
        .filter(|r| matches!(r, Ok(Moved::Moved)))
        .count();
    assert_eq!(moved, 1, "{results:?}");
}

/// 9: gix runs no hooks, so a refusing reference-transaction hook (the
/// one branch protection installs) does not stop sluice's own move.
#[test]
fn a_refusing_hook_does_not_stop_move_to() {
    let repo = Repo::new("hook");
    let old = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let new = rev(repo.branch("unit/u", "bolt/x", &[("b", Some("2"))]));
    let hook = repo.dir().join(".git/hooks/reference-transaction");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!repo
        .git_raw(&["branch", "-f", "bolt/x", new.as_str()])
        .status
        .success());
    assert_eq!(
        refs(&repo).move_to("bolt/x", &old, &new).unwrap(),
        Moved::Moved
    );
}

/// 10, 56, S12: the policy is read from the branch's revision, never from
/// a worktree holding another version of it.
#[test]
fn read_file_reads_the_revision_not_the_worktree() {
    let repo = Repo::new("policy");
    let on_branch = repo.branch(
        "bolt/x",
        "main",
        &[(".config/sluice.toml", Some("on the branch\n"))],
    );
    repo.git(&["switch", "-q", "bolt/x"]);
    repo.write(".config/sluice.toml", "in the worktree\n");
    let r = refs(&repo);
    let read = r
        .read_file(&rev(on_branch.clone()), ".config/sluice.toml")
        .unwrap();
    assert_eq!(read.as_deref(), Some(&b"on the branch\n"[..]));
    assert_eq!(
        r.read_file(&rev(on_branch), ".config/none.toml").unwrap(),
        None
    );
}

/// 24: a landing found under a branch moved again since is in its history.
#[test]
fn contains_answers_ancestry() {
    let repo = Repo::new("contains");
    let r = refs(&repo);
    let a = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let b = rev(repo.branch("unit/u", "bolt/x", &[("b", Some("2"))]));
    let other = rev(repo.branch("unit/o", "main", &[("c", Some("3"))]));
    assert!(r.contains(&b, &a).unwrap());
    assert!(r.contains(&b, &b).unwrap());
    assert!(!r.contains(&a, &b).unwrap());
    assert!(!r.contains(&b, &other).unwrap());
}

/// 121: a revision that is no commit on this host, as another host's
/// landing a remote has since lost, is contained in nothing read here.
#[test]
fn contains_answers_no_for_a_commit_this_host_lacks() {
    let repo = Repo::new("contains-unknown");
    let r = refs(&repo);
    let a = rev(repo.branch("bolt/x", "main", &[("a", Some("1"))]));
    let unknown = Rev::new("0123456789abcdef0123456789abcdef01234567");
    assert!(!r.contains(&a, &unknown).unwrap());
    let blob = rev(repo.git(&["rev-parse", "bolt/x:a"]).trim().to_string());
    assert!(!r.contains(&a, &blob).unwrap());
}

/// The common directory is the same from a linked worktree.
#[test]
fn the_common_dir_is_one_from_every_worktree() {
    let repo = Repo::new("common");
    let linked = repo.dir().with_extension("linked");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        linked.to_str().unwrap(),
        "main",
    ]);
    let from_main = refs(&repo).common_dir().unwrap();
    let from_linked = LocalRefs::open(&linked).unwrap().common_dir().unwrap();
    assert_eq!(from_main, from_linked);
    assert_eq!(from_main, repo.dir().join(".git"));
    let _ = std::fs::remove_dir_all(&linked);
}

/// `git` in `dir`'s repository, its output's lines.
fn lines(repo: &Repo, args: &[&str]) -> Vec<String> {
    repo.git(args).lines().map(str::to_string).collect()
}

/// 129: the default branch, read with no process, is the branch the
/// common directory's HEAD names, as `git symbolic-ref --short -q HEAD`
/// names it there, and none while that HEAD is detached; the branches
/// are what `git for-each-ref refs/heads` lists, packed or loose.
#[test]
fn the_default_branch_and_the_branches_read_as_git_names_them() {
    let repo = Repo::new("named");
    repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    repo.branch("unit/y", "main", &[("b", Some("2"))]);
    let r = refs(&repo);
    // `branch` leaves the worktree detached at main.
    assert!(repo
        .git_raw(&["symbolic-ref", "-q", "HEAD"])
        .stdout
        .is_empty());
    assert_eq!(r.default_branch().unwrap(), None);
    repo.git(&["switch", "-q", "main"]);
    let named = repo.git(&["symbolic-ref", "--short", "-q", "HEAD"]);
    assert_eq!(r.default_branch().unwrap().as_deref(), Some(named.as_str()));
    repo.git(&["pack-refs", "--all"]);
    repo.branch("loose/z", "main", &[("c", Some("3"))]);
    let mut listed = lines(
        &repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    );
    listed.sort();
    let mut read = r.branch_names().unwrap();
    read.sort();
    assert_eq!(read, listed);
    assert_eq!(read, ["bolt/x", "loose/z", "main", "unit/y"]);
}

/// 129: in a bare repository with worktrees, the default branch is the
/// bare repository's HEAD from every worktree, whatever each has checked
/// out, and the branches are the repository's.
#[test]
fn a_bare_repositorys_default_branch_is_its_own_heads() {
    let repo = Repo::new("bare-src");
    repo.branch("bolt/x", "main", &[("a", Some("1"))]);
    repo.git(&["switch", "-q", "main"]);
    let bare = support::Scratch::new("bare.git");
    repo.git(&["clone", "-q", "--bare", ".", bare.path().to_str().unwrap()]);
    let wt = bare.path().join("bolt");
    let git = |args: &[&str]| {
        let mut a = vec!["--git-dir", bare.path().to_str().unwrap()];
        a.extend(args);
        repo.git(&a)
    };
    git(&["worktree", "add", "-q", wt.to_str().unwrap(), "bolt/x"]);
    let r = LocalRefs::open(&wt).unwrap();
    let named = git(&["symbolic-ref", "--short", "-q", "HEAD"]);
    assert_eq!(named, "main");
    assert_eq!(r.default_branch().unwrap().as_deref(), Some("main"));
    let mut listed: Vec<String> = git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .lines()
        .map(str::to_string)
        .collect();
    listed.sort();
    let mut read = r.branch_names().unwrap();
    read.sort();
    assert_eq!(read, listed);
}
