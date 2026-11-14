//! The records seam's `remote refs` profile against a bare repository as
//! the shared remote, from two hosts' clones of it (C.6; 62, 63, 80, 109;
//! ADR 0037). The check-tools tier: run with `--features tools`.

#![cfg(feature = "tools")]

mod support;

use std::collections::BTreeMap;
use std::rc::Rc;

use sluice::profiles::local_files::LocalFiles;
use sluice::profiles::places::Places;
use sluice::profiles::pushed_refs::PushedRefs;
use sluice::profiles::remote_refs::{RemoteRefs, SignerKey, Signing};
use sluice::retry;
use sluice::runs::Runs;
use sluice_core::records::{parse_lines, to_line, ResultKind, ResultLine, RESULT_SCHEMA};
use sluice_seams::{
    read_refused, Changes, Checkout, Enforced, Outcome, Rev, Settings, State, TaskLog, Written,
    KEPT_LOGS,
};
use ssh_key::{Algorithm, PrivateKey};
use support::http::{commands, is_push, listing, lists_for_push, Fake, CUT};
use support::{Repo, Scratch};

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap()
}

fn allowed(principal: &str, k: &PrivateKey) -> SignerKey {
    SignerKey::ssh(&format!(
        "{principal} {}",
        k.public_key().to_openssh().unwrap()
    ))
    .unwrap()
}

/// A host's records of `origin`, signing as `signer` with `k`.
fn host(repo: &Repo, signing: Option<(&str, PrivateKey)>) -> RemoteRefs {
    let pushed = Rc::new(PushedRefs::open_as(repo.dir(), "origin", None, recorded()).unwrap());
    RemoteRefs::new(
        &repo.dir().join(".git"),
        pushed,
        signing.map(|(s, key)| Signing {
            signer: s.into(),
            key,
        }),
    )
}

/// A command's retry policy whose pauses are skipped, as if taken.
fn recorded() -> Rc<retry::Policy> {
    Rc::new(retry::Policy::command().recording())
}

fn put(files: &[(&str, &str)]) -> Changes {
    Changes {
        put: files
            .iter()
            .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
            .collect(),
        ..Changes::default()
    }
}

fn two_hosts(name: &str) -> (Repo, Scratch, Repo) {
    let a = Repo::new(name);
    let bare = a.bare_origin();
    let b = Repo::clone_of(bare.path(), &format!("{name}-b"));
    (a, bare, b)
}

/// 62, 63, 80: the state is one ref on the remote, read from any host,
/// and of two writes from one version exactly one is written.
#[test]
fn two_hosts_write_one_version_and_one_is_refused() {
    let (a, _bare, b) = two_hosts("rr-write");
    let (ha, hb) = (host(&a, None), host(&b, None));
    let empty = ha.read("main").unwrap();
    assert_eq!((empty.version, empty.files.len()), (0, 0));
    {
        let _g = ha.lock("main").unwrap().unwrap();
        assert_eq!(
            ha.write("main", 0, put(&[("branch.json", "{}\n")]))
                .unwrap(),
            Written::Written(1)
        );
    }
    let seen = hb.read("main").unwrap();
    assert_eq!(seen.version, 1);
    assert_eq!(seen.files["branch.json"], b"{}\n");
    assert!(!seen.files.contains_key("version"));

    let at_a = ha.read("main").unwrap().version;
    let at_b = hb.read("main").unwrap().version;
    let wa = ha
        .write("main", at_a, put(&[("branch.json", "{\"a\":1}\n")]))
        .unwrap();
    let wb = hb
        .write("main", at_b, put(&[("branch.json", "{\"b\":1}\n")]))
        .unwrap();
    assert_eq!(wa, Written::Written(2));
    assert_eq!(wb, Written::Refused);
    assert_eq!(
        hb.read("main").unwrap().files["branch.json"],
        b"{\"a\":1}\n"
    );
    assert_eq!(ha.branches().unwrap(), ["main"]);
}

/// A log is appended to, and a finished record is kept beside the live
/// ones, never read with them (130).
#[test]
fn appends_and_finished_records_keep_their_places() {
    let (a, _bare, _b) = two_hosts("rr-append");
    let h = host(&a, None);
    let mut c = put(&[(".finished/7.json", "{\"id\":7}\n")]);
    c.append.insert("log.jsonl".into(), b"one\n".to_vec());
    assert_eq!(h.write("bolt/x", 0, c).unwrap(), Written::Written(1));
    let mut c = Changes::default();
    c.append.insert("log.jsonl".into(), b"two\n".to_vec());
    assert_eq!(h.write("bolt/x", 1, c).unwrap(), Written::Written(2));
    let snap = h.read("bolt/x").unwrap();
    assert_eq!(snap.files["log.jsonl"], b"one\ntwo\n");
    assert!(!snap.files.keys().any(|k| k.starts_with('.')));
    assert_eq!(
        h.read_finished("bolt/x", "7.json").unwrap().unwrap(),
        b"{\"id\":7}\n"
    );
}

/// 51, 130: a write putting a finished record that is there replaces it,
/// read back as written by name and with the rest, on another host too,
/// and the live index is left as it was.
#[test]
fn a_finished_record_put_again_replaces_it_and_leaves_the_index() {
    let (a, _bare, b) = two_hosts("rr-finished-again");
    let (ha, hb) = (host(&a, None), host(&b, None));
    let index = "{\"id\":5,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n";
    let mut c = put(&[(".finished/5.json", "{\"id\":5}\n")]);
    c.append
        .insert("finished.jsonl".into(), index.as_bytes().to_vec());
    assert_eq!(ha.write("bolt/x", 0, c).unwrap(), Written::Written(1));
    let delivered = "{\"id\":5,\"delivered\":\"2026-10-28T09:00:00Z\"}\n";
    let mut c = put(&[(".finished/5.json", delivered)]);
    c.append.insert("log.jsonl".into(), b"delivered\n".to_vec());
    assert_eq!(ha.write("bolt/x", 1, c).unwrap(), Written::Written(2));
    for h in [&ha, &hb] {
        assert_eq!(
            h.read_finished("bolt/x", "5.json").unwrap().unwrap(),
            delivered.as_bytes()
        );
        let all = h.read_all_finished("bolt/x").unwrap();
        assert_eq!(all.keys().collect::<Vec<_>>(), ["5.json"]);
        assert_eq!(all["5.json"], delivered.as_bytes());
        assert_eq!(
            h.read("bolt/x").unwrap().files["finished.jsonl"],
            index.as_bytes()
        );
    }
}

/// 80, 133: a state fetch the remote refuses reads the state last fetched,
/// and says why: the remote, the ref asked for and git's words; the
/// records' other fetches are refused reads too, and the next read that
/// reaches the remote is current again.
#[test]
fn a_refused_state_fetch_is_stale_with_why() {
    let (a, bare, _b) = two_hosts("rr-refused");
    let h = host(&a, None);
    {
        let _g = h.lock("main").unwrap().unwrap();
        h.write("main", 0, put(&[("branch.json", "{}\n")])).unwrap();
    }
    assert_eq!(h.read("main").unwrap().version, 1);
    assert_eq!(h.stale("main"), None);
    let aside = bare.path().with_extension("aside");
    std::fs::rename(bare.path(), &aside).unwrap();
    assert_eq!(h.read("main").unwrap().version, 1, "the state last fetched");
    let stale = h.stale("main").expect("a stale read");
    assert_eq!(stale.remote, "origin");
    assert_eq!(stale.asked, "refs/sluice/state/main");
    assert!(
        stale
            .said
            .contains("does not appear to be a git repository"),
        "{stale:?}"
    );
    for e in [
        h.fetch_heads("main").unwrap_err(),
        h.fetch_candidates("main").unwrap_err(),
    ] {
        let refused = read_refused(&e).expect("a refused read");
        assert_eq!(refused.remote, "origin");
        assert!(refused.asked.starts_with("refs/sluice/"), "{refused:?}");
    }
    std::fs::rename(&aside, bare.path()).unwrap();
    assert_eq!(h.read("main").unwrap().version, 1);
    assert_eq!(h.stale("main"), None);
}

/// 133: this host's refusal of a branch's remote is kept beside what it
/// observed, replaced and removed whole, by either profile; it is never
/// a branch's records.
#[test]
fn each_profile_keeps_a_refusal_beside_the_records() {
    let (a, _bare, _b) = two_hosts("rr-kept");
    let h = host(&a, None);
    let local = LocalFiles::new(&a.dir().join(".git"));
    {
        let _g = local.lock("bolt/x").unwrap().unwrap();
        local
            .write("bolt/x", 0, put(&[("branch.json", "{}\n")]))
            .unwrap();
    }
    let at = [
        a.dir()
            .join(".git/sluice-remote/origin/refused/bolt/x.json"),
        a.dir().join(".git/sluice/.refused/bolt/x.json"),
    ];
    for (s, at) in [(&h as &dyn State, &at[0]), (&local, &at[1])] {
        assert_eq!(s.refusal("bolt/x").unwrap(), None);
        s.keep_refusal("bolt/x", Some(b"{\"rounds\":1}\n")).unwrap();
        assert_eq!(std::fs::read(at).unwrap(), b"{\"rounds\":1}\n");
        s.keep_refusal("bolt/x", Some(b"{\"rounds\":2}\n")).unwrap();
        assert_eq!(
            s.refusal("bolt/x").unwrap().as_deref(),
            Some(&b"{\"rounds\":2}\n"[..])
        );
        s.keep_refusal("main", Some(b"{}\n")).unwrap();
        s.keep_refusal("bolt/x", None).unwrap();
        assert_eq!(s.refusal("bolt/x").unwrap(), None);
        assert!(!at.exists());
        s.keep_refusal("bolt/x", None).unwrap();
    }
    assert_eq!(local.branches().unwrap(), ["bolt/x"]);
    assert!(!local
        .read("bolt/x")
        .unwrap()
        .files
        .keys()
        .any(|k| k.contains("refused")));
}

/// Submission ids are numbered once across every host.
#[test]
fn ids_are_numbered_once_across_hosts() {
    let (a, _bare, b) = two_hosts("rr-ids");
    let (ha, hb) = (host(&a, None), host(&b, None));
    assert_eq!(ha.allocate_id("main").unwrap(), 1);
    assert_eq!(hb.allocate_id("bolt/x").unwrap(), 2);
    assert_eq!(ha.allocate_id("main").unwrap(), 3);
}

fn ran(task: &str, outcome: Outcome) -> ResultLine {
    ResultLine {
        schema: RESULT_SCHEMA.into(),
        at: "2026-10-19T12:00:00Z".into(),
        kind: ResultKind::Ran,
        task: task.into(),
        hash: format!("{task}-key"),
        outcome,
        host: Some("box".into()),
        executor: Some(Enforced {
            kind: "container".into(),
            confined: true,
            processors: None,
            memory: None,
            network: None,
            docker: false,
            grant: None,
        }),
        branch: "main".into(),
        combination: Rev::new("c0ffee"),
        worktree: None,
        system: Some("linux-x64".into()),
        signer: None,
        took_ms: None,
        attempts: None,
    }
}

fn lines(ls: &[ResultLine]) -> Vec<u8> {
    ls.iter().flat_map(to_line).collect()
}

/// 78, 109: a host signs its runs and adds them to its signer's ref;
/// another host reads them verified, by signer, and a record no listed key
/// signed counts as nothing.
#[test]
fn signed_runs_are_read_verified_on_another_host() {
    let (a, _bare, b) = two_hosts("rr-signed");
    let (box_key, stranger) = (key(), key());
    let ha = host(&a, Some(("box", box_key.clone())));
    let mut replay = ran("c:test", Outcome::Pass);
    replay.kind = ResultKind::Replayed;
    replay.host = None;
    replay.executor = None;
    ha.record_results(&lines(&[
        ran("a:test", Outcome::Pass),
        ran("b:test", Outcome::Fail),
        replay,
    ]))
    .unwrap();

    let hb = host(&b, None);
    let signers = BTreeMap::from([("box".into(), allowed("exec@build-1", &box_key))]);
    let all: Vec<ResultLine> = parse_lines(&hb.results_with(&signers).unwrap()).unwrap();
    // The replay is signed with its run, as a replay (108; core.9).
    assert!(all.iter().any(|l| l.kind == ResultKind::Replayed
        && l.task == "c:test"
        && l.signer.as_deref() == Some("box")));
    let read: Vec<ResultLine> = all
        .into_iter()
        .filter(|l| l.kind == ResultKind::Ran)
        .collect();
    assert_eq!(read.len(), 2);
    assert!(read.iter().all(|l| l.signer.as_deref() == Some("box")
        && l.host.as_deref() == Some("exec@build-1")
        && l.system.as_deref() == Some("linux-x64")
        && l.executor.as_ref().is_some_and(|e| e.confined)));
    let b_test = read.iter().find(|l| l.task == "b:test").unwrap();
    assert_eq!(b_test.outcome, Outcome::Fail);
    // A key the policy does not list proves nothing (S18).
    let hc = host(&b, None);
    let strangers = BTreeMap::from([("box".into(), allowed("other", &stranger))]);
    assert!(
        parse_lines::<ResultLine>(&hc.results_with(&strangers).unwrap())
            .unwrap()
            .is_empty()
    );
    // Nor does a profile asked with no list.
    assert!(parse_lines::<ResultLine>(
        &hc.results_for("main", &sluice_seams::ResultAsk::every())
            .unwrap()
    )
    .unwrap()
    .is_empty());
}

/// 19, 138, design D3 (the check-grant spec's "Two grants in one
/// combination"): one combination's results run under two grants, held
/// to `network = "localhost"` and the container engine, are signed as two
/// statements, and each reads back on another host with those limits and
/// the grant it was run under.
#[test]
fn a_signed_runs_grant_is_read_from_its_statement() {
    let (a, bare, b) = two_hosts("rr-grant");
    let box_key = key();
    let under = |task: &str, grant: &str| {
        let mut l = ran(task, Outcome::Pass);
        l.executor = Some(Enforced {
            network: Some(sluice_seams::Network::Localhost),
            docker: true,
            grant: Some(grant.into()),
            ..l.executor.unwrap()
        });
        l
    };
    host(&a, Some(("box", box_key.clone())))
        .record_results(&lines(&[
            under("a:test", "sb1:g1"),
            under("b:test", "sb1:g2"),
            under("c:test", "sb1:g1"),
        ]))
        .unwrap();
    let signed = a.git(&[
        "-C",
        bare.path().to_str().unwrap(),
        "ls-tree",
        "-r",
        "--name-only",
        "refs/sluice/runs/box",
    ]);
    assert_eq!(signed.lines().count(), 2, "{signed}");

    let signers = BTreeMap::from([("box".into(), allowed("box", &box_key))]);
    let read: Vec<ResultLine> =
        parse_lines(&host(&b, None).results_with(&signers).unwrap()).unwrap();
    let mut held: Vec<(String, Option<sluice_seams::Network>, bool, Option<String>)> = read
        .into_iter()
        .map(|l| {
            let e = l.executor.unwrap();
            (l.task, e.network, e.docker, e.grant)
        })
        .collect();
    held.sort_by(|x, y| x.0.cmp(&y.0));
    let localhost = Some(sluice_seams::Network::Localhost);
    assert_eq!(
        held,
        [
            ("a:test".into(), localhost, true, Some("sb1:g1".into())),
            ("b:test".into(), localhost, true, Some("sb1:g2".into())),
            ("c:test".into(), localhost, true, Some("sb1:g1".into())),
        ]
    );
}

/// 78, rule core.22: a statement verified with one branch's key for a
/// signer is not read as verified when another branch's key for that
/// signer is asked, in the same profile, and is again when the first is.
#[test]
fn a_statement_verified_with_one_key_is_unverified_with_another() {
    let (a, _bare, b) = two_hosts("rr-two-keys");
    let (key_a, key_b) = (key(), key());
    host(&a, Some(("box", key_a.clone())))
        .record_results(&lines(&[ran("a:test", Outcome::Pass)]))
        .unwrap();
    let h = host(&b, None);
    let read = |signers: &BTreeMap<String, SignerKey>| -> Vec<String> {
        parse_lines::<ResultLine>(&h.results_with(signers).unwrap())
            .unwrap()
            .into_iter()
            .map(|l| l.task)
            .collect()
    };
    let with_a = BTreeMap::from([("box".into(), allowed("box", &key_a))]);
    let with_b = BTreeMap::from([("box".into(), allowed("box", &key_b))]);
    assert_eq!(read(&with_a), ["a:test"]);
    assert!(
        read(&with_b).is_empty(),
        "verified with key A, asked with B"
    );
    assert_eq!(read(&with_a), ["a:test"]);
}

/// Could-not-run-1 116, rule core.11: a result set aside on one host is
/// read set aside on every host, and two set-asides at once both stay.
#[test]
fn a_set_aside_is_read_on_another_host() {
    let (a, _bare, b) = two_hosts("rr-aside");
    let ha = host(&a, None);
    let hb = host(&b, None);
    ha.set_aside(b"{\"task\":\"a:test\"}").unwrap();
    hb.set_aside(b"{\"task\":\"b:test\"}").unwrap();
    for h in [&ha, &hb] {
        let text = String::from_utf8(h.set_asides().unwrap()).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("a:test") && text.contains("b:test"), "{text}");
    }
}

/// 109: two hosts of one signer each add their runs, and neither loses
/// the other's.
#[test]
fn two_hosts_of_one_signer_keep_both_adds() {
    let (a, _bare, b) = two_hosts("rr-adds");
    let (ka, kb) = (key(), key());
    host(&a, Some(("sessions", ka.clone())))
        .record_results(&lines(&[ran("a:test", Outcome::Pass)]))
        .unwrap();
    host(&b, Some(("sessions", kb.clone())))
        .record_results(&lines(&[ran("b:test", Outcome::Pass)]))
        .unwrap();
    let both = SignerKey::ssh(&format!(
        "a {}\nb {}\n",
        ka.public_key().to_openssh().unwrap(),
        kb.public_key().to_openssh().unwrap()
    ))
    .unwrap();
    let reader = host(&a, None);
    let signers = BTreeMap::from([("sessions".into(), both)]);
    let mut tasks: Vec<String> = parse_lines::<ResultLine>(&reader.results_with(&signers).unwrap())
        .unwrap()
        .into_iter()
        .map(|l| l.task)
        .collect();
    tasks.sort();
    assert_eq!(tasks, ["a:test", "b:test"]);
}

/// 78: a host with no signing key records no run on the remote, and says
/// why.
#[test]
fn a_host_with_no_signing_key_records_nothing_and_says_so() {
    let (a, _bare, _b) = two_hosts("rr-unsigned");
    let err = host(&a, None)
        .record_results(&lines(&[ran("a:test", Outcome::Pass)]))
        .unwrap_err();
    assert!(format!("{err:#}").contains("[signing]"), "{err:#}");
}

/// 20: a failed candidate's outputs are kept on the remote too, for the
/// last `KEPT_LOGS` candidates of the branch.
#[test]
fn logs_are_kept_on_the_remote_for_the_last_candidates() {
    let (a, bare, _b) = two_hosts("rr-logs");
    let h = host(&a, None);
    for n in 1..=(KEPT_LOGS + 2) {
        let kept = h
            .keep_logs(
                "main",
                &format!("c{n}"),
                &[TaskLog {
                    task: "a:test".into(),
                    text: format!("run {n}\n"),
                }],
            )
            .unwrap();
        assert!(std::fs::read_to_string(&kept[0].text)
            .unwrap()
            .contains(&format!("run {n}")));
    }
    let out = support::git_command()
        .arg("--git-dir")
        .arg(bare.path())
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/sluice/logs/main/",
        ])
        .output()
        .unwrap();
    let refs = String::from_utf8(out.stdout).unwrap();
    assert_eq!(refs.lines().count(), KEPT_LOGS);
    assert!(!refs.contains("refs/sluice/logs/main/c1\n"));
    assert!(refs.contains(&format!("refs/sluice/logs/main/c{}\n", KEPT_LOGS + 2)));
}

/// Design D10: the first host to take a branch whose records moved to its
/// remote pushes its own queue there as the starting state; another host
/// whose queue was not first leaves its queued submissions behind, for its
/// round to submit again.
#[test]
fn the_first_hosts_queue_moves_and_the_others_is_left_behind() {
    use sluice::profiles::branches::Branches;
    use sluice::profiles::local_files::LocalFiles;
    use sluice::profiles::records::Records;

    let k = key();
    let a = Repo::new("rr-move");
    a.write(
        ".config/sluice.toml",
        &format!(
            "[signers.box]\nssh = \"box {}\"\n\n[branches.main]\nremote = \"origin\"\ntrust = [\"box\"]\n",
            k.public_key().to_openssh().unwrap()
        ),
    );
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "signers"]);
    let bare = a.bare_origin();
    let b = Repo::clone_of(bare.path(), "rr-move-b");
    // Each host's own queue, as its sluice wrote it before the move.
    let own = |r: &Repo, id: u64| {
        let s = LocalFiles::new(&r.dir().join(".git"));
        let _g = s.lock("main").unwrap().unwrap();
        let doc = format!(
            "{{\"schema\":\"sluice/submission@1\",\"id\":{id},\"branch\":\"main\",\"state\":\"queued\"}}\n"
        );
        s.write("main", 0, put(&[(&format!("submissions/{id}.json"), &doc)]))
            .unwrap();
    };
    own(&a, 5);
    own(&b, 9);

    let records = |r: &Repo| {
        Records::new(
            &r.dir().join(".git"),
            Branches::open_as(r.dir(), None, recorded()).unwrap(),
            None,
        )
    };
    let ra = records(&a);
    drop(ra.lock("main").unwrap().unwrap());
    let moved = ra.read("main").unwrap();
    assert!(moved.files.contains_key("submissions/5.json"));
    assert!(ra.left_behind("main").unwrap().is_empty());

    let rb = records(&b);
    drop(rb.lock("main").unwrap().unwrap());
    assert!(!rb
        .read("main")
        .unwrap()
        .files
        .contains_key("submissions/9.json"));
    let left = rb.left_behind("main").unwrap();
    assert_eq!(left.len(), 1);
    assert!(String::from_utf8_lossy(&left[0]).contains("\"id\":9"));
    rb.forget_left_behind("main", &[9]).unwrap();
    assert!(rb.left_behind("main").unwrap().is_empty());
}

/// 134, 122, 77: a host's sign of life whose push was cut after it was
/// sent is pushed once, and stands when the remote's ref holds the commit
/// it pushed.
#[test]
fn a_sign_of_life_cut_after_it_was_sent_stands() {
    let repo = Repo::new("rr-beat-cut");
    let held = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let fake = {
        let held = held.clone();
        Fake::start(move |r| {
            if lists_for_push(r) {
                listing("git-receive-pack", &[])
            } else if is_push(r) {
                let (_, new, _) = commands(r).remove(0);
                *held.lock().unwrap() = Some(new);
                (CUT, vec![], String::new())
            } else {
                let held = held.lock().unwrap().clone();
                match &held {
                    Some(at) => listing("git-upload-pack", &[("refs/sluice/hosts/h", at)]),
                    None => listing("git-upload-pack", &[]),
                }
            }
        })
    };
    repo.git(&[
        "remote",
        "add",
        "origin",
        &format!("{}/o/r.git", fake.url()),
    ]);
    let h = host(&repo, None);
    h.beat("h", b"{}").unwrap();
    let pushes = fake.requests().iter().filter(|r| is_push(r)).count();
    assert_eq!(pushes, 1, "one push");
    assert!(held.lock().unwrap().is_some(), "the remote holds the beat");
}

/// 134, 62: the state's leased push refused at its lease, another host
/// having written first, is one push, and the write is refused as before.
#[test]
fn a_state_write_refused_at_the_lease_is_one_push() {
    use std::os::unix::fs::PermissionsExt;
    let (a, _bare, b) = two_hosts("rr-lease-once");
    let (ha, hb) = (host(&a, None), host(&b, None));
    let at_a = ha.read("main").unwrap().version;
    let at_b = hb.read("main").unwrap().version;
    // Every push of b's runs this, once.
    let log = b.dir().join(".git/pushes.log");
    let pack = b.dir().join(".git/receive-pack");
    std::fs::write(
        &pack,
        format!(
            "#!/bin/sh\necho push >> '{}'\nexec git receive-pack \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&pack, std::fs::Permissions::from_mode(0o755)).unwrap();
    b.git(&[
        "config",
        "remote.origin.receivepack",
        pack.to_str().unwrap(),
    ]);
    let wa = ha
        .write("main", at_a, put(&[("branch.json", "{\"a\":1}\n")]))
        .unwrap();
    let wb = hb
        .write("main", at_b, put(&[("branch.json", "{\"b\":1}\n")]))
        .unwrap();
    assert_eq!(wa, Written::Written(1));
    assert_eq!(wb, Written::Refused);
    let pushes = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(pushes.lines().count(), 1, "one push: {pushes}");
}

/// Count each fetch `repo` makes from `origin` in a file, one line a
/// fetch: git runs this in place of `git upload-pack` on the remote.
fn count_fetches(repo: &Repo) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let log = repo.dir().join(".git/fetches.log");
    let pack = repo.dir().join(".git/upload-pack");
    std::fs::write(
        &pack,
        format!(
            "#!/bin/sh\necho fetch >> '{}'\nexec git upload-pack \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&pack, std::fs::Permissions::from_mode(0o755)).unwrap();
    repo.git(&["config", "remote.origin.uploadpack", pack.to_str().unwrap()]);
    log
}

/// `rev` is an object `repo` holds.
fn holds(repo: &Repo, rev: &str) -> bool {
    repo.git_raw(&["cat-file", "-e", &format!("{rev}^{{commit}}")])
        .status
        .success()
}

/// The revision `name` holds in `repo`, or `None` when it has none.
fn ref_at(repo: &Repo, name: &str) -> Option<String> {
    let out = repo.git_raw(&["rev-parse", "--verify", "-q", name]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 64, 129: a tick's ask of the remote brings, in its one fetch, the
/// branch into its tracking ref, the work another host submitted and the
/// combinations formed on the branch, with the records.
#[test]
fn a_ticks_ask_brings_the_branch_and_the_work_in_one_fetch() {
    let (a, _bare, b) = two_hosts("rr-with-branch");
    let (ha, hb) = (host(&a, None), host(&b, None));
    {
        let _g = hb.lock("main").unwrap().unwrap();
        hb.write("main", 0, put(&[("branch.json", "{}\n")]))
            .unwrap();
    }
    let moved = b.push_change("moved", "on the remote\n");
    let head = b.branch("unit/b", "main", &[("work", Some("b's\n"))]);
    hb.publish_head("main", &Rev::new(head.clone())).unwrap();
    let combination = b.branch("combined", "main", &[("both", Some("1 and 2\n"))]);
    hb.publish_candidate("main", "c1", &Rev::new(combination.clone()))
        .unwrap();
    assert!(!holds(&a, &head) && !holds(&a, &combination));

    let fetches = count_fetches(&a);
    ha.refresh().unwrap();
    let read = ha.read_held("main").unwrap();
    assert_eq!(read.version, 1);
    assert_eq!(read.files["branch.json"], b"{}\n");
    assert_eq!(ha.stale("main"), None);
    let fetched = std::fs::read_to_string(&fetches).unwrap();
    assert_eq!(fetched.lines().count(), 1, "one fetch: {fetched}");
    assert_eq!(
        ref_at(&a, "refs/remotes/origin/main").as_deref(),
        Some(moved.as_str())
    );
    assert!(holds(&a, &head), "the head submitted from b");
    assert!(holds(&a, &combination), "the combination b formed");
}

/// 82, C.6 (record 0118): a combination another host opened, which this
/// host lacks, is fetched by its own ref, the others left on the remote,
/// and a claimed run's place is made at it here; a combination the
/// remote no longer holds fetches nothing.
#[test]
fn a_combination_only_the_remote_holds_is_fetched_and_a_place_made_at_it() {
    let (a, _bare, b) = two_hosts("rr-one-candidate");
    let (ha, hb) = (host(&a, None), host(&b, None));
    let combination = b.branch("combined", "main", &[("both", Some("1 and 2\n"))]);
    hb.publish_candidate("main", "c7", &Rev::new(combination.clone()))
        .unwrap();
    let other = b.branch("other", "main", &[("other", Some("3\n"))]);
    hb.publish_candidate("main", "c8", &Rev::new(other.clone()))
        .unwrap();
    assert!(!holds(&a, &combination) && !holds(&a, &other));

    ha.fetch_candidate("main", "c7").unwrap();
    assert!(holds(&a, &combination), "the combination asked for");
    assert!(!holds(&a, &other), "and no other");
    assert_eq!(
        ref_at(&a, "refs/sluice-fetched/origin/combinations/main/c7").as_deref(),
        Some(combination.as_str())
    );
    ha.fetch_candidate("main", "c9").unwrap();
    assert_eq!(
        ref_at(&a, "refs/sluice-fetched/origin/combinations/main/c9"),
        None
    );

    let common = a.dir().join(".git");
    let user = Scratch::short();
    let places = Places::new(
        &common,
        Runs::new(&common, env!("CARGO_BIN_EXE_sluice").into()),
    )
    .with_scratch_root(user.path().join("u"));
    let name = "sluice/try/main/7";
    let place = places
        .materialize(name, &Rev::new(combination.clone()), &Settings::new())
        .unwrap();
    let at = support::git_command()
        .current_dir(&place.path)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&at.stdout).trim(), combination);
    places.release(name).unwrap();
}

/// 64, 129: where the remote no longer holds the branch, the tick's ask
/// brings its records all the same, and leaves no revision of the branch
/// to name as the remote's (124).
#[test]
fn a_branch_the_remote_lacks_still_gives_the_records() {
    let (a, bare, _b) = two_hosts("rr-no-branch");
    let h = host(&a, None);
    {
        let _g = h.lock("main").unwrap().unwrap();
        h.write("main", 0, put(&[("branch.json", "{}\n")])).unwrap();
    }
    assert!(ref_at(&a, "refs/remotes/origin/main").is_some());
    let out = support::git_command()
        .arg("--git-dir")
        .arg(bare.path())
        .args(["update-ref", "-d", "refs/heads/main"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    h.refresh().unwrap();
    let read = h.read_held("main").unwrap();
    assert_eq!(read.version, 1);
    assert_eq!(read.files["branch.json"], b"{}\n");
    assert_eq!(h.stale("main"), None);
    assert_eq!(ref_at(&a, "refs/remotes/origin/main"), None);
}

/// 64, 80, 129: a remote out of reach at a tick's ask leaves the records
/// as last fetched, and the read says it is stale, in the remote's words;
/// the branch's tracking ref stays as it was, which a reader then names
/// no revision from.
#[test]
fn a_remote_out_of_reach_gives_the_records_last_fetched() {
    let (a, bare, _b) = two_hosts("rr-with-branch-stale");
    let h = host(&a, None);
    {
        let _g = h.lock("main").unwrap().unwrap();
        h.write("main", 0, put(&[("branch.json", "{}\n")])).unwrap();
    }
    h.refresh().unwrap();
    assert_eq!(h.read_held("main").unwrap().version, 1);
    let tracked = ref_at(&a, "refs/remotes/origin/main");
    let aside = bare.path().with_extension("aside");
    std::fs::rename(bare.path(), &aside).unwrap();
    let refreshed = h.refresh();
    let read = h.read_held("main").unwrap();
    std::fs::rename(&aside, bare.path()).unwrap();
    refreshed.unwrap();
    assert_eq!(read.version, 1, "the records last fetched");
    let stale = h.stale("main").expect("a stale read");
    assert_eq!(stale.remote, "origin");
    assert!(
        stale
            .said
            .contains("does not appear to be a git repository"),
        "{stale:?}"
    );
    assert_eq!(ref_at(&a, "refs/remotes/origin/main"), tracked);
}

/// Two hosts whose `main` and `release` keep their records on origin:
/// this host's repository, the remote, and the other host's records of
/// it, each branch's records written once from the other host; and a
/// count of the fetches this host makes of origin (129).
struct Asked {
    a: Repo,
    bare: Scratch,
    other: RemoteRefs,
    _b: Repo,
    fetches: std::path::PathBuf,
}

impl Asked {
    fn new(name: &str) -> Asked {
        let k = key();
        let a = Repo::new(name);
        a.write(
            ".config/sluice.toml",
            &format!(
                "[signers.box]\nssh = \"box {}\"\n\n\
                 [branches.main]\nremote = \"origin\"\ntrust = [\"box\"]\n\n\
                 [branches.release]\nremote = \"origin\"\ntrust = [\"box\"]\n",
                k.public_key().to_openssh().unwrap()
            ),
        );
        a.git(&["add", "-A"]);
        a.git(&["commit", "-q", "-m", "signers"]);
        a.git(&["branch", "release"]);
        let bare = a.bare_origin();
        let b = Repo::clone_of(bare.path(), &format!("{name}-b"));
        let other = host(&b, None);
        for branch in ["main", "release"] {
            other
                .write(branch, 0, put(&[("branch.json", "{}\n")]))
                .unwrap();
        }
        // Each fetch from origin, as origin's side of it counts it.
        let fetches = a.dir().join(".git/fetches");
        let wrapper = a.dir().join(".git/upload-pack");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\necho fetch >> {}\nexec git upload-pack \"$@\"\n",
                fetches.display()
            ),
        )
        .unwrap();
        a.git(&[
            "config",
            "remote.origin.uploadpack",
            &format!("sh {}", wrapper.display()),
        ]);
        Asked {
            a,
            bare,
            other,
            _b: b,
            fetches,
        }
    }

    /// This host's records, as a command binds them.
    fn records(&self) -> sluice::profiles::records::Records {
        sluice::profiles::records::Records::new(
            &self.a.dir().join(".git"),
            sluice::profiles::branches::Branches::open_as(self.a.dir(), None, recorded()).unwrap(),
            None,
        )
    }

    fn fetched(&self) -> usize {
        std::fs::read_to_string(&self.fetches)
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// The other host's next write of `branch`'s records.
    fn write_elsewhere(&self, branch: &str) {
        let at = self.other.read(branch).unwrap().version;
        let mut c = Changes::default();
        c.append.insert("log.jsonl".into(), b"{}\n".to_vec());
        assert!(matches!(
            self.other.write(branch, at, c).unwrap(),
            Written::Written(_)
        ));
    }
}

/// 129: a tick's ask of origin is one fetch for both branches' records,
/// each read from what it brought with no fetch of its own; a branch
/// whose records are added on origin is listed after the next ask.
#[test]
fn one_ask_brings_every_branchs_records() {
    let s = Asked::new("rr-ask");
    let r = s.records();
    r.refresh().unwrap();
    let before = s.fetched();
    r.refresh().unwrap();
    assert_eq!(s.fetched(), before + 1, "one fetch for two branches");
    for branch in ["main", "release"] {
        let held = r.read_held(branch).unwrap();
        assert_eq!(held.version, 1, "{branch}");
        assert_eq!(held.files["branch.json"], b"{}\n", "{branch}");
        assert_eq!(r.stale(branch), None, "{branch}");
    }
    assert_eq!(s.fetched(), before + 1, "held, nothing fetched");
    assert_eq!(r.branches().unwrap(), ["main", "release"]);

    s.other
        .write("bolt/y", 0, put(&[("branch.json", "{}\n")]))
        .unwrap();
    assert_eq!(r.branches().unwrap(), ["main", "release"]);
    r.refresh().unwrap();
    assert_eq!(r.branches().unwrap(), ["bolt/y", "main", "release"]);
}

/// 129, 80, 133: an ask origin refuses leaves both branches read as last
/// fetched, each stale in the remote's words, and fails nothing; the next
/// ask, answered, ends it.
#[test]
fn a_refused_ask_leaves_every_branch_as_last_fetched() {
    let s = Asked::new("rr-ask-refused");
    let r = s.records();
    r.refresh().unwrap();
    s.write_elsewhere("main");
    let away = s.bare.path().with_extension("away");
    std::fs::rename(s.bare.path(), &away).unwrap();
    let refreshed = r.refresh();
    let held: Vec<_> = ["main", "release"]
        .iter()
        .map(|b| (r.read_held(b).unwrap().version, r.stale(b)))
        .collect();
    std::fs::rename(&away, s.bare.path()).unwrap();
    refreshed.unwrap();
    for (version, stale) in held {
        assert_eq!(version, 1, "as last fetched");
        let stale = stale.expect("stale");
        assert_eq!(stale.remote, "origin");
        assert!(
            stale
                .said
                .contains("does not appear to be a git repository"),
            "{stale:?}"
        );
    }
    r.refresh().unwrap();
    assert_eq!(r.stale("main"), None);
    assert_eq!(r.read_held("main").unwrap().version, 2);
}

/// 129: a round's read of a branch still fetches its records, whatever
/// the tick's ask brought.
#[test]
fn a_rounds_read_still_fetches_its_branch() {
    let s = Asked::new("rr-ask-round");
    let r = s.records();
    r.refresh().unwrap();
    s.write_elsewhere("main");
    let before = s.fetched();
    assert_eq!(r.read_held("main").unwrap().version, 1);
    assert_eq!(s.fetched(), before);
    assert_eq!(r.read("main").unwrap().version, 2);
    assert_eq!(s.fetched(), before + 1);
    assert_eq!(r.read_held("main").unwrap().version, 2);
}

/// A policy keeping `main` and `release` on origin, naming each signer in
/// `signers` with its keys, every one trusted.
fn signers_policy(signers: &[(&str, &[&PrivateKey])]) -> String {
    let mut out = String::new();
    for (name, keys) in signers {
        let lines: Vec<String> = keys
            .iter()
            .map(|k| format!("{name} {}", k.public_key().to_openssh().unwrap()))
            .collect();
        out += &format!("[signers.{name}]\nssh = \"{}\"\n\n", lines.join("\\n"));
    }
    let trust: Vec<String> = signers.iter().map(|(n, _)| format!("\"{n}\"")).collect();
    for b in ["main", "release"] {
        out += &format!(
            "[branches.{b}]\nremote = \"origin\"\ntrust = [{}]\n\n",
            trust.join(", ")
        );
    }
    out
}

/// A host whose `main` names `main`'s signers and whose `release` names
/// `release`'s, both on origin, and origin.
fn branches_on_origin(
    name: &str,
    main: &[(&str, &[&PrivateKey])],
    release: &[(&str, &[&PrivateKey])],
) -> (Repo, Scratch) {
    let a = Repo::new(name);
    a.write(".config/sluice.toml", &signers_policy(main));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "main's signers"]);
    let bare = a.bare_origin();
    let policy = signers_policy(release);
    a.branch("release", "main", &[(".config/sluice.toml", Some(&policy))]);
    a.git(&["push", "-q", "origin", "release"]);
    (a, bare)
}

/// `ls` signed as `signer` with `k`, added to its ref on origin.
fn sign(repo: &Repo, signer: &str, k: &PrivateKey, ls: &[ResultLine]) {
    host(repo, Some((signer, k.clone())))
        .record_results(&lines(ls))
        .unwrap();
}

/// A pass of `task` on a combination of `branch`.
fn passed_on(task: &str, branch: &str) -> ResultLine {
    let mut l = ran(task, Outcome::Pass);
    l.branch = branch.into();
    l
}

/// This host's records, as a command binds them, verifying nothing it
/// signs.
fn records(repo: &Repo) -> sluice::profiles::records::Records {
    sluice::profiles::records::Records::new(
        &repo.dir().join(".git"),
        sluice::profiles::branches::Branches::open_as(repo.dir(), None, recorded()).unwrap(),
        None,
    )
}

/// The tasks of the signed runs `branch`'s gate reads, in order.
fn signed(r: &sluice::profiles::records::Records, branch: &str) -> Vec<String> {
    let mut tasks: Vec<String> = parse_lines::<ResultLine>(
        &r.results_for(branch, &sluice_seams::ResultAsk::every())
            .unwrap(),
    )
    .unwrap()
    .into_iter()
    .filter(|l| l.signer.is_some())
    .map(|l| l.task)
    .collect();
    tasks.sort();
    tasks
}

/// 78, 27, rule core.22: `main` lists key A for `box` and `release` key
/// B; each branch's runs are verified with its own key, whichever branch
/// one process reads first.
#[test]
fn each_branch_reads_runs_with_its_own_key_whichever_is_read_first() {
    let (ka, kb) = (key(), key());
    let (a, _bare) = branches_on_origin("rr-own-keys", &[("box", &[&ka])], &[("box", &[&kb])]);
    sign(&a, "box", &ka, &[passed_on("a:test", "main")]);
    sign(&a, "box", &kb, &[passed_on("b:test", "release")]);
    for order in [["main", "release"], ["release", "main"]] {
        let r = records(&a);
        let read: BTreeMap<&str, Vec<String>> = order.iter().map(|b| (*b, signed(&r, b))).collect();
        assert_eq!(read["main"], ["a:test"], "read {order:?}");
        assert_eq!(read["release"], ["b:test"], "read {order:?}");
    }
}

/// 78, 87, rule core.22: a host signing as `box` with key A has runs that
/// count for `main`, whose policy lists A, and none for `release`, whose
/// policy lists B for `box`; a host that signs nothing has none.
#[test]
fn a_hosts_runs_count_only_where_the_branch_lists_its_key() {
    let (ka, kb) = (key(), key());
    let (a, _bare) = branches_on_origin("rr-own-claims", &[("box", &[&ka])], &[("box", &[&kb])]);
    let keys = Scratch::new("rr-own-claims-key");
    let path = keys.path().join("signing");
    ka.write_openssh_file(&path, ssh_key::LineEnding::LF)
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let r = sluice::profiles::records::Records::new(
        &a.dir().join(".git"),
        sluice::profiles::branches::Branches::open_as(a.dir(), None, recorded()).unwrap(),
        Some(sluice::instance::Signing {
            signer: "box".into(),
            key: path,
        }),
    );
    let (main, release) = (Rev::new(a.rev("main")), Rev::new(a.rev("release")));
    assert!(r.verifies_own("main", &main).unwrap());
    assert!(!r.verifies_own("release", &release).unwrap());
    assert!(!records(&a).verifies_own("main", &main).unwrap());
    assert_eq!(r.lists_read(), 2, "no read again at a revision read");
    assert!(r.verifies_own("main", &main).unwrap());
    assert_eq!(r.lists_read(), 2);
}

/// 78, 56, rule core.31: a signer only the second branch names counts for
/// it; a key added to `main` on origin while a process runs counts once
/// that process fetched the revision, its policy read again once for the
/// move and not again while the revision stands.
#[test]
fn a_signer_only_the_second_branch_names_and_a_key_added_later_count() {
    let (ka, kb, kc) = (key(), key(), key());
    let (a, bare) = branches_on_origin(
        "rr-later-key",
        &[("box", &[&ka])],
        &[("box", &[&ka]), ("ci", &[&kc])],
    );
    sign(&a, "box", &ka, &[passed_on("a:test", "main")]);
    sign(&a, "box", &kb, &[passed_on("b:test", "main")]);
    sign(&a, "ci", &kc, &[passed_on("c:test", "release")]);
    let r = records(&a);
    assert_eq!(signed(&r, "main"), ["a:test"]);
    assert_eq!(signed(&r, "release"), ["a:test", "c:test"]);
    let read = r.lists_read();
    assert_eq!(signed(&r, "main"), ["a:test"]);
    assert_eq!(r.lists_read(), read, "nothing moved, nothing read");

    let other = Repo::clone_of(bare.path(), "rr-later-key-b");
    other.push_change(
        ".config/sluice.toml",
        &signers_policy(&[("box", &[&ka, &kb])]),
    );
    a.git(&["fetch", "-q", "origin"]);
    assert_eq!(signed(&r, "main"), ["a:test", "b:test"]);
    assert_eq!(r.lists_read(), read + 1, "read again for the move");
    assert_eq!(signed(&r, "main"), ["a:test", "b:test"]);
    assert_eq!(r.lists_read(), read + 1, "not again at the same revision");
    assert_eq!(signed(&r, "release"), ["a:test", "c:test"]);
}

/// 124: `release` is routed to origin, which holds no `release`, and the
/// host's own `release` names `box`: no signed run counts for it, while
/// `main`, which origin holds, reads box's run.
#[test]
fn a_branch_its_remote_does_not_hold_reads_no_signed_run() {
    let ka = key();
    let a = Repo::new("rr-unheld");
    a.write(".config/sluice.toml", &signers_policy(&[("box", &[&ka])]));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "signers"]);
    let _bare = a.bare_origin();
    a.branch("release", "main", &[("notes", Some("only here\n"))]);
    assert_eq!(ref_at(&a, "refs/remotes/origin/release"), None);
    sign(&a, "box", &ka, &[passed_on("a:test", "release")]);
    let r = records(&a);
    assert!(r.route("release").unwrap().is_some(), "routed to origin");
    assert!(signed(&r, "release").is_empty());
    assert_eq!(signed(&r, "main"), ["a:test"]);
}

/// Rule core.14 (ADR 0028): a round of a branch whose records are on its
/// remote keeps its run's tasks under the lock the records seam took for
/// it, and keeps none without that lock (swb-1, 2026-11-12: every round of
/// a branch on its remote with a run testing refused at its tasks,
/// `keeping <branch>'s tasks without holding its lock`, so it wrote
/// nothing, its verdicts and releases among it).
#[test]
fn a_branch_on_its_remote_keeps_its_tasks_under_the_lock_its_records_took() {
    use sluice_seams::{TaskProgress, TaskState};

    let a = Repo::new("rr-tasks");
    a.write(
        ".config/sluice.toml",
        &signers_policy(&[("box", &[&key()])]),
    );
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "signers"]);
    let _bare = a.bare_origin();
    let r = records(&a);
    assert!(r.route("main").unwrap().is_some(), "routed to origin");
    let tasks = [TaskProgress {
        target: "a:test".into(),
        state: TaskState::Running,
        started: None,
        ended: None,
        exit: None,
    }];
    assert!(r.put_progress("main", "c1", Some(&tasks)).is_err());
    {
        let _g = r.lock("main").unwrap().expect("main's lock is free");
        r.put_progress("main", "c1", Some(&tasks)).unwrap();
        assert_eq!(r.progress("main").unwrap()["c1"], tasks);
    }
    assert!(r.put_progress("main", "c1", None).is_err());
    assert_eq!(r.progress("main").unwrap()["c1"], tasks);
    {
        let _g = r.wait_lock("main").unwrap();
        r.put_progress("main", "c1", None).unwrap();
    }
    assert!(r.progress("main").unwrap().is_empty());
}

/// 78, rule core.22: `scratch` keeps its records in the host's own files
/// and its policy names no signer; `main` on origin names `box`. None of
/// box's runs is read for `scratch`, whichever branch was read first.
#[test]
fn a_branch_in_the_hosts_own_files_reads_with_its_own_list() {
    let ka = key();
    let a = Repo::new("rr-own-files");
    a.write(".config/sluice.toml", &signers_policy(&[("box", &[&ka])]));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "signers"]);
    let _bare = a.bare_origin();
    a.branch(
        "scratch",
        "main",
        &[(".config/sluice.toml", Some("[branches.scratch]\n"))],
    );
    sign(&a, "box", &ka, &[passed_on("a:test", "scratch")]);
    let r = records(&a);
    assert_eq!(signed(&r, "main"), ["a:test"]);
    assert!(r.route("scratch").unwrap().is_none(), "in the host's files");
    assert!(signed(&r, "scratch").is_empty());
}

/// Record 0114, rule core.41: with origin's records open for `main`,
/// `scratch`, whose records are the host's own and which holds #1 to #3,
/// takes its next number from the host's count, never origin's, and so
/// never one it holds; `main` takes origin's (switchboard-kit's main on
/// 2026-11-12, given #52 and #53 again from GitHub's count while the
/// host's stood at 267).
#[test]
fn a_branch_in_the_hosts_own_files_numbers_from_the_hosts_count() {
    let ka = key();
    let a = Repo::new("rr-own-count");
    a.write(".config/sluice.toml", &signers_policy(&[("box", &[&ka])]));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "signers"]);
    let _bare = a.bare_origin();
    a.branch(
        "scratch",
        "main",
        &[(".config/sluice.toml", Some("[branches.scratch]\n"))],
    );
    let own = LocalFiles::new(&a.dir().join(".git"));
    let held: Vec<(String, String)> = (0..3)
        .map(|_| own.allocate_id("scratch").unwrap())
        .map(|id| {
            (
                format!("submissions/{id}.json"),
                format!(
                    "{{\"schema\":\"sluice/submission@1\",\"id\":{id},\"branch\":\"scratch\",\"state\":\"queued\"}}\n"
                ),
            )
        })
        .collect();
    {
        let _g = own.lock("scratch").unwrap().unwrap();
        let files: Vec<(&str, &str)> = held.iter().map(|(p, d)| (&p[..], &d[..])).collect();
        own.write("scratch", 0, put(&files)).unwrap();
    }

    let r = records(&a);
    assert!(r.route("main").unwrap().is_some(), "origin's records open");
    assert!(r.route("scratch").unwrap().is_none(), "in the host's files");
    let next = r.allocate_id("scratch").unwrap();
    assert_eq!(next, 4, "the host's count");
    let holds = r.read("scratch").unwrap();
    assert!(!holds
        .files
        .contains_key(&format!("submissions/{next}.json")));
    assert_eq!(r.allocate_id("main").unwrap(), 1, "origin's count");
    assert_eq!(r.allocate_id("scratch").unwrap(), 5);
}
