//! sluice on two hosts sharing one remote, end to end through the CLI:
//! a branch whose policy names signers keeps its state on the remote, a
//! session on one host submits and goes away, and the other host lands
//! the submission (C.6; 72, 78, 82, 111). A bare repository on this host
//! is the remote. Needs git, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use ssh_key::{Algorithm, LineEnding, PrivateKey};
use support::{Repo, Scratch};

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// One host: its clone of the remote, its own config and state, and its
/// signing key.
struct Host {
    repo: Repo,
    config: Scratch,
    state: Scratch,
    sessions: Scratch,
    /// The git every command it runs starts, counting its starts (129).
    git: support::CountingGit,
}

impl Host {
    fn new(repo: Repo, name: &str, signer: &str, key: &PrivateKey) -> Host {
        let config = Scratch::new(&format!("{name}-config"));
        let path = config.path().join("signing");
        key.write_openssh_file(&path, LineEnding::LF).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::create_dir_all(config.path().join("sluice")).unwrap();
        std::fs::write(
            config.path().join("sluice/config.toml"),
            format!(
                "[signing]\nsigner = \"{signer}\"\nkey = {:?}\n\n[peers]\nname = \"{name}\"\n",
                path.display().to_string()
            ),
        )
        .unwrap();
        Host {
            repo,
            state: Scratch::new(&format!("{name}-state")),
            sessions: Scratch::new(&format!("{name}-sessions")),
            config,
            git: support::CountingGit::new(),
        }
    }

    /// The binary in `dir`, this setup's host. The host process its
    /// commands start passes only when asked, so the rounds that run are
    /// the ones the test's commands ask for (design D14).
    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
        support::isolated(&mut c)
            .current_dir(dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        self.git.on(&mut c);
        c
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.command(self.repo.dir()).args(args).output().unwrap()
    }

    /// A session on `unit/<name>` off `main`, one commit of its own,
    /// submitted to `main` without waiting; returns its id.
    fn submit(&self, name: &str) -> u64 {
        self.submit_to(name, "main")
    }

    /// A session on `unit/<name>` off `branch`, one commit of its own,
    /// submitted to `branch` without waiting; returns its id.
    fn submit_to(&self, name: &str, branch: &str) -> u64 {
        let dir: PathBuf = self.sessions.path().join(name);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            branch,
        ]);
        std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
        let git = |args: &[&str]| {
            let mut a = vec!["-C", dir.to_str().unwrap()];
            a.extend(args);
            self.repo.git(&a)
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", name]);
        let out = self
            .command(&dir)
            .args(["submit", branch, "--no-wait", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["id"].as_u64().unwrap()
    }

    /// Until every host process its commands started has ended, as one
    /// does once nothing is in play and nothing follows it, at most ten
    /// seconds: its log names each start's pid ending (135; design D6).
    fn host_process_ended(&self) {
        let log = self.state.path().join("sluice/host/log");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let started: Vec<&str> = text
                .lines()
                .filter_map(|l| l.split(" started, pid ").nth(1))
                .filter_map(|rest| rest.split(';').next())
                .collect();
            let ended = |pid: &&str| {
                text.lines()
                    .any(|l| l.ends_with(&format!("sluice host {pid} ends")))
            };
            if started.iter().all(ended) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "a host process never ended: {text}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn state_of(&self, id: u64) -> String {
        let out = self.sluice(&["status", &id.to_string(), "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["state"].as_str().unwrap().to_string()
    }
}

fn line(principal: &str, k: &PrivateKey) -> String {
    format!("{principal} {}", k.public_key().to_openssh().unwrap())
}

/// Two hosts over one bare remote, under a policy naming each one's
/// signer and trusting those in `trust`; the first host's and the
/// second's, and the remote.
fn two_hosts(name: &str, trust: &str) -> (Host, Host, Scratch) {
    let (ka, kb) = (
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
    );
    let a = Repo::new(&format!("{name}-a"));
    a.write(
        ".config/sluice.toml",
        &format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
             [signers.a]\nssh = \"{}\"\n\n[signers.b]\nssh = \"{}\"\n\n\
             [branches.main]\ntimeout = \"1m\"\nremote = \"origin\"\ntrust = {trust}\n",
            line("a", &ka),
            line("b", &kb)
        ),
    );
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "sluice's policy"]);
    let origin = a.bare_origin();
    let b = Repo::clone_of(origin.path(), &format!("{name}-b"));
    (
        Host::new(a, &format!("{name}-a"), "a", &ka),
        Host::new(b, &format!("{name}-b"), "b", &kb),
        origin,
    )
}

fn refs_under(origin: &Scratch, prefix: &str) -> String {
    let out = support::git_command()
        .arg("--git-dir")
        .arg(origin.path())
        .args(["for-each-ref", "--format=%(refname)", prefix])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// 75, 82, 78: the host that keeps the branch signs as no one its gate
/// trusts, so the other host's `sluice run`, a standing executor, claims
/// the run, fetches the combination,
/// runs it and signs it, and the first lands it.
#[test]
fn a_run_is_claimed_and_run_by_the_other_host() {
    let (host_a, host_b, origin) = two_hosts("claims", "[\"b\"]");
    let id = host_a.submit("x");
    let out = host_a.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        refs_under(&origin, "refs/sluice/combinations/").contains("refs/sluice/combinations/main/")
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    while host_a.state_of(id) != "landed" {
        for (h, args) in [
            (&host_b, ["run", "--rounds", "1"]),
            (&host_a, ["step", "main", "--json"]),
        ] {
            let out = h.sluice(&args);
            if !out.status.success() {
                let refs = h
                    .repo
                    .git(&["for-each-ref", "--format=%(objectname) %(refname)"]);
                let doc = String::from_utf8_lossy(
                    &h.repo
                        .git_raw(&["show", "refs/sluice-fetched/origin/state/main:branch.json"])
                        .stdout,
                )
                .into_owned();
                panic!("{}\nrefs:\n{refs}\nstate:\n{doc}", text(&out));
            }
        }
        assert!(
            Instant::now() < deadline,
            "never landed: {}",
            host_a.state_of(id)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(refs_under(&origin, "refs/sluice/runs/").contains("refs/sluice/runs/b"));
    assert!(!refs_under(&origin, "refs/sluice/runs/").contains("refs/sluice/runs/a"));
    assert!(refs_under(&origin, "refs/sluice/hosts/").lines().count() >= 1);
    let out = host_a.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(refs_under(&origin, "refs/sluice/combinations/"), "");
}

/// 111, 82, 78: a session's host submits and stops; another host, with no
/// session of its own, lands the submission, signing its run; the first
/// host reads it landed from the remote when it is back.
#[test]
fn a_submission_lands_by_another_host() {
    let (host_a, host_b, origin) = two_hosts("peers", "[\"a\", \"b\"]");

    let id = host_a.submit("x");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let out = host_b.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        if host_b.state_of(id) == "landed" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "never landed: {}",
            host_b.state_of(id)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(host_a.state_of(id), "landed");
    // The landing is on the remote's branch, made by the other host.
    let landed = support::git_command()
        .arg("--git-dir")
        .arg(origin.path())
        .args(["log", "-1", "--format=%s", "main"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&landed.stdout).contains(&format!("#{id}")));
    // The head is gone from the remote by the round after its submission
    // landed (82).
    let out = host_b.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    let heads = support::git_command()
        .arg("--git-dir")
        .arg(origin.path())
        .args(["for-each-ref", "refs/sluice/heads/"])
        .output()
        .unwrap();
    assert!(
        heads.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&heads.stdout)
    );
}

/// 51, rule core.11; outcome-deliveries spec, "A branch whose state is
/// on its remote": a submission from worktree `unit/x` on the first host
/// lands there on `main`, whose state is on origin; on the second host, a
/// worktree on `unit/x` collects its landing with `sluice inbox --json`,
/// since its delivery is among the branch's state on origin, and hands it
/// over by `inbox` there (record 0102, D3).
#[test]
fn an_outcome_on_the_remote_is_collected_on_another_host() {
    let (host_a, host_b, _origin) = two_hosts("peers-inbox", "[\"a\", \"b\"]");
    let id = host_a.submit("x");
    let deadline = Instant::now() + Duration::from_secs(60);
    while host_a.state_of(id) != "landed" {
        assert!(
            Instant::now() < deadline,
            "never landed: {}",
            host_a.state_of(id)
        );
        let out = host_a.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        std::thread::sleep(Duration::from_millis(200));
    }

    let dir = host_b.sessions.path().join("x");
    host_b.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/x",
        dir.to_str().unwrap(),
        "main",
    ]);
    let out = host_b
        .command(&dir)
        .args(["inbox", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let outcomes = v["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 1, "{v}");
    assert_eq!(outcomes[0]["id"], id, "{v}");
    assert_eq!(outcomes[0]["state"], "landed", "{v}");
    assert_eq!(
        outcomes[0]["delivery"]["key"],
        format!("{id}:landed").as_str(),
        "{v}"
    );
    assert_eq!(outcomes[0]["delivery"]["reader"], "unit/x", "{v}");
    let handed = outcomes[0]["delivery"]["handed"].as_array().unwrap();
    assert_eq!(handed.last().unwrap()["by"], "inbox", "{v}");
}

/// 124, 27; record 0069: two hosts share origin, whose `main` names
/// `[signers.box]` with box's key and `remote = "origin"`; the second
/// host's own `main`, moved past the protection, adds a
/// `[[checkout.cache]]` table to that policy, which sluice refuses. The
/// second host still reads `main`'s state from origin, and its rounds,
/// signing as box, count box's runs by the key origin's policy names and
/// land the first host's submission on origin.
#[test]
fn a_host_whose_own_main_is_refused_counts_the_remotes_signers() {
    let (ka, kbox) = (
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
    );
    let policy = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
         [signers.box]\nssh = \"{}\"\n\n\
         [branches.main]\ntimeout = \"1m\"\nremote = \"origin\"\ntrust = [\"box\"]\n",
        line("box", &kbox)
    );
    let a = Repo::new("refused-own-a");
    a.write(".config/sluice.toml", &policy);
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "sluice's policy"]);
    let origin = a.bare_origin();
    let b = Repo::clone_of(origin.path(), "refused-own-b");
    let theirs = b.rev("refs/remotes/origin/main");
    // As an older sluice left it, past the protection.
    b.write(
        ".config/sluice.toml",
        &format!("{policy}\n[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n"),
    );
    b.git(&["add", "-A"]);
    b.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-q",
        "-m",
        "a cache",
    ]);
    assert_ne!(b.rev("main"), theirs);
    let host_a = Host::new(a, "refused-own-a", "a", &ka);
    let host_b = Host::new(b, "refused-own-b", "box", &kbox);

    let id = host_a.submit("x");
    let out = host_b.sluice(&["status", "main", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(host_b.state_of(id), "queued");
    let deadline = Instant::now() + Duration::from_secs(60);
    while host_b.state_of(id) != "landed" {
        let out = host_b.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            Instant::now() < deadline,
            "never landed: {}",
            host_b.state_of(id)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(host_a.state_of(id), "landed");
    assert!(refs_under(&origin, "refs/sluice/runs/").contains("refs/sluice/runs/box"));
    let landed = support::git_command()
        .arg("--git-dir")
        .arg(origin.path())
        .args(["log", "-1", "--format=%s", "main"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&landed.stdout).contains(&format!("#{id}")));
}

/// Two hosts over one bare remote, both signing as `box`: `main`'s policy
/// lists the first host's key for `box`, `release`'s the second's. The
/// first host's and the second's, and the remote.
fn own_keys(name: &str) -> (Host, Host, Scratch) {
    let (ka, kb) = (
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
    );
    let policy = |k: &PrivateKey| {
        format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
             [signers.box]\nssh = \"{}\"\n\n\
             [branches.main]\ntimeout = \"10m\"\nremote = \"origin\"\ntrust = [\"box\"]\n\n\
             [branches.release]\ntimeout = \"10m\"\nremote = \"origin\"\ntrust = [\"box\"]\n",
            line("box", k)
        )
    };
    let a = Repo::new(&format!("{name}-a"));
    a.write(".config/sluice.toml", &policy(&ka));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "sluice's policy"]);
    a.git(&["checkout", "-q", "-b", "release"]);
    a.write(".config/sluice.toml", &policy(&kb));
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "release's key"]);
    a.git(&["checkout", "-q", "main"]);
    let origin = a.bare_origin();
    let b = Repo::clone_of(origin.path(), &format!("{name}-b"));
    (
        Host::new(a, &format!("{name}-a"), "box", &ka),
        Host::new(b, &format!("{name}-b"), "box", &kb),
        origin,
    )
}

/// Steps `hosts` take, `main` then `release` each, until `id` lands.
fn land(hosts: &[&Host], id: u64) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while hosts[0].state_of(id) != "landed" {
        for h in hosts {
            for branch in ["main", "release"] {
                let out = h.sluice(&["step", branch]);
                assert!(out.status.success(), "{}", text(&out));
            }
        }
        assert!(
            Instant::now() < deadline,
            "never landed: {}",
            hosts[0].state_of(id)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `id` on `branch` waits with its check missing: not failed, and no run
/// of it was taken, its log on `origin` holding no claim and no result.
fn missing(host: &Host, origin: &Scratch, id: u64, branch: &str) {
    let state = host.state_of(id);
    assert!(state == "queued" || state == "testing", "{id} is {state}");
    let out = host.sluice(&["status", &id.to_string(), "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["failed"].as_array().is_none_or(|f| f.is_empty()),
        "{id} read a fail: {v:#}"
    );
    let log = support::git_command()
        .arg("--git-dir")
        .arg(origin.path())
        .args(["show", &format!("refs/sluice/state/{branch}:log.jsonl")])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    for event in ["\"claimed\"", "\"ran\"", "\"uncounted\""] {
        assert!(!log.contains(event), "{branch}'s log holds {event}:\n{log}");
    }
}

/// 78, 87, 28, S18; rule core.22: two hosts share origin and both sign as
/// `box`; `main`'s policy lists host a's key for `box` and `release`'s
/// lists host b's; host a submits to both. A host takes a branch's run
/// only where the branch's own policy lists the key it signs with: host
/// b's steps land `release`'s submission and leave `main`'s queued, its
/// check missing, until host a's steps run it.
#[test]
fn a_host_runs_a_branch_only_where_its_policy_lists_the_hosts_key() {
    let (host_a, host_b, origin) = own_keys("own-key");
    let x = host_a.submit_to("x", "main");
    let y = host_a.submit_to("y", "release");
    land(&[&host_b], y);
    missing(&host_b, &origin, x, "main");
    land(&[&host_a, &host_b], x);
}

/// 78, 87; rule core.22: the same two hosts, host a stepping first: its
/// steps land `main`'s submission and leave `release`'s queued, its check
/// missing, until host b's steps run it.
#[test]
fn the_other_host_runs_the_other_branch() {
    let (host_a, host_b, origin) = own_keys("other-key");
    let x = host_a.submit_to("x", "main");
    let y = host_a.submit_to("y", "release");
    land(&[&host_a], x);
    missing(&host_a, &origin, y, "release");
    land(&[&host_b, &host_a], y);
}

/// One host whose `main` and `release` keep their records on origin, each
/// with one submission landed, and the remote.
fn two_branches_landed(name: &str) -> (Host, Scratch) {
    let k = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap();
    let repo = Repo::new(name);
    repo.write(
        ".config/sluice.toml",
        &format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
             [signers.a]\nssh = \"{}\"\n\n\
             [branches.main]\ntimeout = \"1m\"\nremote = \"origin\"\ntrust = [\"a\"]\n\n\
             [branches.release]\ntimeout = \"1m\"\nremote = \"origin\"\ntrust = [\"a\"]\n",
            line("a", &k)
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "sluice's policy"]);
    repo.git(&["branch", "release"]);
    let origin = repo.bare_origin();
    let host = Host::new(repo, name, "a", &k);
    for (n, branch) in [("m", "main"), ("r", "release")] {
        let id = host.submit_to(n, branch);
        let deadline = Instant::now() + Duration::from_secs(60);
        while host.state_of(id) != "landed" {
            let out = host.sluice(&["step", branch]);
            assert!(out.status.success(), "{}", text(&out));
            assert!(Instant::now() < deadline, "{branch}: {}", host.state_of(id));
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    (host, origin)
}

/// The starts of git among `starts` that are the tick's one ask of
/// origin.
fn asks(starts: &[String]) -> usize {
    starts
        .iter()
        .filter(|l| {
            l.starts_with("--git-dir") && l.contains(" fetch ") && l.contains("refs/sluice/state/*")
        })
        .count()
}

/// reader-tick, "Two branches' records on one remote": with nothing
/// queued, each pass of the host process after its first starts git once,
/// for one fetch from origin (129). Each `sluice run` starts a host process
/// of its own, the last one having ended, so the two runs read alike at
/// its start and differ by their passes alone (135).
#[test]
fn each_pass_asks_origin_once_for_both_branches() {
    let (host, _origin) = two_branches_landed("ticks-two");
    let run = |rounds: &str| -> Vec<String> {
        host.host_process_ended();
        let before = host.git.count();
        let out = host
            .command(host.repo.dir())
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["run", "--rounds", rounds])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        host.git.starts()[before..].to_vec()
    };
    let one = run("1");
    let six = run("6");
    assert_eq!(six.len(), one.len() + 5, "one: {one:?}; six: {six:?}");
    assert_eq!(asks(&six), asks(&one) + 5, "{six:?}");
}

/// A watch of `host`'s repository, the reader; ended when dropped.
struct Watching {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Watching {
    fn start(host: &Host) -> Watching {
        let mut child = host
            .command(host.repo.dir())
            .args(["overview", "--json", "--watch"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        let (send, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufRead::lines(out).map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Watching { child, lines }
    }

    fn next(&self, within: Duration) -> Option<serde_json::Value> {
        let line = self.lines.recv_timeout(within).ok()?;
        Some(serde_json::from_str(&line).unwrap())
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// reader-tick, "Two branches' records on one remote", the watch: each
/// tick of the reader's watch after its first starts git once, for one
/// fetch from origin (129).
#[test]
fn each_watch_tick_asks_origin_once_for_both_branches() {
    let (host, _origin) = two_branches_landed("ticks-watch");
    let watch = Watching::start(&host);
    let first = watch.next(Duration::from_secs(10)).expect("a first line");
    let branches: Vec<&str> = first["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["branch"].as_str().unwrap())
        .collect();
    assert_eq!(branches, ["main", "release"], "{first}");
    let before = host.git.count();
    std::thread::sleep(Duration::from_millis(2500));
    let starts = host.git.starts()[before..].to_vec();
    assert!(!starts.is_empty(), "no tick asked");
    assert_eq!(asks(&starts), starts.len(), "{starts:?}");
}

/// reader-tick, "The remote refuses the tick's ask": the watch's line
/// shows both branches as last fetched, each saying it could not refresh
/// in the remote's words, and the next tick asks again (80, 129, 133).
#[test]
fn a_refused_ask_shows_both_branches_as_last_fetched() {
    let (host, origin) = two_branches_landed("ticks-refused");
    let watch = Watching::start(&host);
    let first = watch.next(Duration::from_secs(10)).expect("a first line");
    let at: Vec<serde_json::Value> = first["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["at"].clone())
        .collect();
    let away = origin.path().with_extension("away");
    std::fs::rename(origin.path(), &away).unwrap();
    let refused = watch.next(Duration::from_secs(10));
    let before = host.git.count();
    std::thread::sleep(Duration::from_millis(2500));
    let again = host.git.starts()[before..].to_vec();
    std::fs::rename(&away, origin.path()).unwrap();
    let refused = refused.expect("a line once origin refused");
    let branches = refused["branches"].as_array().unwrap();
    assert_eq!(branches.len(), 2, "{refused}");
    for (b, at) in branches.iter().zip(&at) {
        assert_eq!(&b["at"], at, "as last fetched: {refused}");
        let stale = b["stale"].as_str().unwrap_or_default();
        assert!(
            stale.contains("origin") && stale.contains("does not appear to be a git repository"),
            "{refused}"
        );
    }
    assert!(asks(&again) >= 1, "the next tick asks again: {again:?}");
}

/// Every line of `starts` that asks a remote anything.
fn remote_calls(starts: &[String]) -> Vec<String> {
    starts
        .iter()
        .filter(|l| {
            [" fetch ", " ls-remote ", " push ", "upload-pack"]
                .iter()
                .any(|c| l.contains(c))
        })
        .cloned()
        .collect()
}

/// 129, 64 (design D16): with `main` and `release` on origin and a
/// submission queued on `main`, a one-shot overview beside the host
/// process's current reading asks origin nothing: it reads every branch as
/// the host holds it since that reading's one ask, `main`'s queue among
/// them.
#[test]
fn a_one_shot_overview_beside_a_current_reading_asks_origin_nothing() {
    let (host, _origin) = two_branches_landed("overview-current");
    host.submit("queued");
    // The host process reads the repository for a one-shot read.
    let out = host.sluice(&["status", "main"]);
    assert!(out.status.success(), "{}", text(&out));

    let before = host.git.count();
    let out = host.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let calls = remote_calls(&host.git.starts()[before..]);
    assert!(calls.is_empty(), "{calls:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let main = v["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["branch"] == "main")
        .cloned()
        .unwrap_or_else(|| panic!("main in {v}"));
    assert_eq!(main["depth"], 1, "{v}");
}

/// 129 (design D16): a status beside the host process's current reading
/// asks origin nothing, and says the queue as the host holds it.
#[test]
fn a_status_beside_a_current_reading_asks_origin_nothing() {
    let (host, _origin) = two_branches_landed("status-current");
    host.submit("queued");
    let out = host.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));

    let before = host.git.count();
    let out = host.sluice(&["status", "main", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let calls = remote_calls(&host.git.starts()[before..]);
    assert!(calls.is_empty(), "{calls:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["submissions"].as_array().map(Vec::len), Some(1), "{v}");
}

/// 129, 69 (design D17): with `main`'s records on origin holding several
/// signed run records, a host process started afresh and passing the
/// repository ten times, and `sluice status <n>` run three times, each
/// record was verified once, by the host process, and no command read one
/// itself.
#[test]
fn a_signed_record_is_verified_once() {
    let (host, origin) = two_branches_landed("verified-once");
    let mut landed = Vec::new();
    for n in ["m2", "m3"] {
        let id = host.submit_to(n, "main");
        let deadline = Instant::now() + Duration::from_secs(60);
        while host.state_of(id) != "landed" {
            let out = host.sluice(&["step", "main"]);
            assert!(out.status.success(), "{}", text(&out));
            assert!(Instant::now() < deadline, "#{id}: {}", host.state_of(id));
            std::thread::sleep(Duration::from_millis(200));
        }
        landed.push(id);
    }
    let listed = support::git_command()
        .args(["--git-dir", origin.path().to_str().unwrap()])
        .args(["ls-tree", "-r", "--name-only", "refs/sluice/runs/a"])
        .output()
        .unwrap();
    let records = String::from_utf8_lossy(&listed.stdout).lines().count();
    assert!(records >= 4, "{}", String::from_utf8_lossy(&listed.stderr));

    // A host process started afresh, kept by a follow, so one index reads
    // what every command after asks.
    host.host_process_ended();
    let out = host.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let repository = doc["repository"].as_str().unwrap().to_string();
    let client = sluice::host_process::client::Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(&host.command(host.repo.dir())),
    )
    .unwrap();
    let _kept = client
        .follow(
            Path::new(&repository),
            sluice::host_process::protocol::FollowKind::Wait,
        )
        .unwrap();

    let out = host.sluice(&["run", "--rounds", "10"]);
    assert!(out.status.success(), "{}", text(&out));
    for _ in 0..3 {
        let out = host.sluice(&["status", &landed[1].to_string(), "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let said = text(&out);
        assert!(!said.contains("reads them itself"), "{said}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["state"], "landed", "{v}");
        assert!(v["checks"].as_array().is_some_and(|c| !c.is_empty()), "{v}");
    }
    let log = std::fs::read_to_string(client.paths().log()).unwrap();
    let verified: usize = log
        .lines()
        .filter_map(|l| l.split(": verified ").nth(1))
        .filter_map(|rest| rest.split(' ').next())
        .map(|n| n.parse::<usize>().unwrap())
        .sum();
    assert_eq!(verified, records, "{log}");
}

/// satisfies: record 0122, 26, rule core.48 (design D5, D6); the
/// submission-wait spec's "The queue moves while a wait follows" and
/// "Carried under another number": host b's own queue of `main` holds #1
/// while `main`'s policy names no signers; a landing brings signers and
/// `remote = "origin"`, and host a, with no records of its own, has #1 on
/// origin. `sluice wait 1 --json` on b follows b's #1 as b's host process
/// moves the queue, which carries it to origin as #2: the wait follows #2,
/// never origin's own #1, until it lands, exits 0, and its document is
/// #2's record naming `was` 1 and `carried_from` this host's own files.
#[test]
fn a_wait_follows_its_submission_to_its_queues_new_home() {
    let (ka, kb) = (
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
    );
    let unsigned = "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
                    [branches.main]\ntimeout = \"1m\"\nremote = \"origin\"\n";
    let a = Repo::new("homes-wait-a");
    a.write(".config/sluice.toml", unsigned);
    a.git(&["add", "-A"]);
    a.git(&["commit", "-q", "-m", "sluice's policy"]);
    let origin = a.bare_origin();
    let b = Repo::clone_of(origin.path(), "homes-wait-b");
    let host_a = Host::new(a, "homes-wait-a", "a", &ka);
    let host_b = Host::new(b, "homes-wait-b", "b", &kb);

    let own = host_b.submit("y");
    assert_eq!(own, 1);
    assert!(refs_under(&origin, "refs/sluice/state/").is_empty());
    // A landing brings signers and origin; b fetches it.
    let signed = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
         [signers.a]\nssh = \"{}\"\n\n[signers.b]\nssh = \"{}\"\n\n\
         [branches.main]\ntimeout = \"1m\"\nremote = \"origin\"\ntrust = [\"a\", \"b\"]\n",
        line("a", &ka),
        line("b", &kb)
    );
    host_a.repo.push_change(".config/sluice.toml", &signed);
    host_b.repo.git(&["fetch", "-q", "origin"]);
    let theirs = host_a.submit("x");
    assert_eq!(theirs, 1, "origin's own #1");
    assert!(refs_under(&origin, "refs/sluice/state/").contains("refs/sluice/state/main"));

    let out = host_b
        .command(host_b.repo.dir())
        .args(["wait", "1", "--json", "--timeout", "90s"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["was"], 1, "{doc}");
    assert_eq!(doc["carried_from"], "here", "{doc}");
    assert_ne!(doc["id"], 1, "never origin's own #1: {doc}");
    assert_eq!(doc["from"]["worktree"], "unit/y", "{doc}");
    let now = doc["id"].as_u64().unwrap();
    assert_eq!(host_b.state_of(now), "landed");
    host_b.host_process_ended();
}
