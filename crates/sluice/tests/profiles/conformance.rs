//! The conformance suite's scenarios that need no fault, on the real
//! profiles: a scratch repository holding a moon workspace of three
//! projects, places made with git worktrees, checks run by moon under the
//! supervisor, branches moved through local refs and records in local
//! files. Needs moon 2.6 or later on the path, so it is in the
//! check-tools tier.
//!
//! Each session's agent is the rig's to name, as `SLUICE_AGENT` names it
//! for a real session: tests share one process, and its environment.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Result;
use sluice::instance::{HostCache, Instance};
use sluice::profiles::branches::Branches;
use sluice::profiles::checks::Checks;
use sluice::profiles::places::Places;
use sluice::profiles::records::Records;
use sluice::profiles::Bound;
use sluice::profiles::Drivers;
use sluice_conformance::{policy, shared, Rig, BRANCH};
use sluice_core::records::State as Entry;
use sluice_core::{Seams, Sluice, Step, SystemClock};
use sluice_seams::{
    Branch, Checkout, Delivery, Driver, Formed, HostCopy, LandingCommit, Moved, Notification,
    Place, Progress, Rev, RunHandle, RunRequest, Session, Settings, Submitter, TaskLog,
    TaskProgress, Tests,
};
use support::{Repo, Scratch};

/// The check every project's test runs: it fails when a rule in the rig's
/// rules file names it and the file the rule names holds its content, and
/// goes on until it is ended when a rule names it with the path `-`, or
/// with the content `-` and a file the place holds.
const CHECK: &str = r#"# $1: the task.
tab=$(printf '\t')
while IFS="$tab" read -r task path content; do
  if [ "$task" = "$1" ] && [ "$path" = "-" ]; then
    echo "$1: hangs"
    exec sleep 600
  fi
  if [ "$task" = "$1" ] && [ "$content" = "-" ] && [ -f "$path" ]; then
    echo "$1: $path is there, and it hangs"
    exec sleep 600
  fi
  if [ "$task" = "$1" ] && [ -f "$path" ] && [ "$(cat "$path")" = "$content" ]; then
    echo "$1: $path holds $content"
    exit 1
  fi
done <RULES
"#;

/// The places the core made and released, and each release it asked for.
#[derive(Default)]
struct Made {
    made: Vec<String>,
    released: Vec<String>,
    asked: Vec<String>,
    /// Each place whose working copy was gone and whose stored release
    /// ran (123).
    gone: Vec<String>,
}

struct RecordedPlaces {
    inner: Places,
    log: RefCell<Made>,
}

impl Checkout for RecordedPlaces {
    fn materialize(&self, name: &str, rev: &Rev, checkout: &Settings) -> Result<Place> {
        let place = self.inner.materialize(name, rev, checkout)?;
        self.log.borrow_mut().made.push(name.into());
        Ok(place)
    }

    fn made(&self) -> Result<Vec<String>> {
        self.inner.made()
    }

    fn release(&self, name: &str) -> Result<()> {
        self.log.borrow_mut().asked.push(name.into());
        self.inner.release(name)?;
        self.log.borrow_mut().released.push(name.into());
        Ok(())
    }

    fn gone(&self, name: &str) -> Result<bool> {
        self.inner.gone(name)
    }

    fn free(&self, name: &str) -> Result<()> {
        self.inner.free(name)?;
        self.log.borrow_mut().released.push(name.into());
        Ok(())
    }

    fn release_gone(&self, name: &str) -> Result<sluice_seams::GoneRelease> {
        let gone = self.inner.release_gone(name)?;
        let mut log = self.log.borrow_mut();
        log.released.push(name.into());
        if gone.ran {
            log.gone.push(name.into());
        }
        Ok(gone)
    }

    fn remove_unnamed_scratch(&self) -> Result<Vec<std::path::PathBuf>> {
        self.inner.remove_unnamed_scratch()
    }

    fn adopt(&self, worktree: &str, branch: &str, checkout: &Settings, at: &str) -> Result<Place> {
        self.inner.adopt(worktree, branch, checkout, at)
    }

    fn session_places(&self) -> Result<Vec<sluice_seams::SessionPlace>> {
        self.inner.session_places()
    }

    fn ran(&self, name: &str, last: &sluice_seams::LastResult) -> Result<()> {
        self.inner.ran(name, last)
    }

    fn checked_out(&self) -> Result<Vec<sluice_seams::CheckedOut>> {
        self.inner.checked_out()
    }

    fn adopted(&self, worktree: &str) -> Result<Option<sluice_seams::Adopted>> {
        self.inner.adopted(worktree)
    }

    fn release_session(
        &self,
        name: &str,
        policy: Option<&sluice_seams::ReleasePolicy>,
    ) -> Result<sluice_seams::Released> {
        self.log.borrow_mut().asked.push(name.into());
        self.inner.release_session(name, policy)
    }

    fn preview(
        &self,
        name: &str,
        rev: &Rev,
        branch: &str,
        checkout: &Settings,
        addresses: &std::collections::BTreeMap<String, String>,
        at: &str,
    ) -> Result<(Place, sluice_seams::PreviewPlace)> {
        self.inner
            .preview(name, rev, branch, checkout, addresses, at)
    }

    fn preview_run(&self, name: &str, run: &RunHandle) -> Result<()> {
        self.inner.preview_run(name, run)
    }

    fn previews(&self) -> Result<Vec<sluice_seams::PreviewPlace>> {
        self.inner.previews()
    }

    fn release_preview(&self, name: &str, failed: Option<&str>, retries: u32) -> Result<bool> {
        self.log.borrow_mut().asked.push(name.into());
        self.inner.release_preview(name, failed, retries)
    }
}

struct RecordedChecks {
    inner: Checks,
    requests: RefCell<Vec<RunRequest>>,
    started: RefCell<Vec<Rev>>,
}

impl Tests for RecordedChecks {
    fn start(&self, request: &RunRequest) -> Result<RunHandle> {
        self.requests.borrow_mut().push(request.clone());
        let run = self.inner.start(request)?;
        self.started.borrow_mut().push(request.revision.clone());
        Ok(run)
    }

    fn progress(&self, run: &RunHandle) -> Result<Progress> {
        self.inner.progress(run)
    }

    fn unmet(&self, limits: &sluice_seams::Limits) -> Result<Option<String>> {
        self.inner.unmet(limits)
    }

    fn switched(&self, run: &RunHandle) -> Result<Option<String>> {
        self.inner.switched(run)
    }

    fn refused(&self, run: &RunHandle) -> Result<sluice_seams::Refusals> {
        self.inner.refused(run)
    }

    fn bound(&self) -> Result<usize> {
        self.inner.bound()
    }

    fn platform(&self) -> Result<sluice_seams::Platform> {
        self.inner.platform()
    }

    fn running(&self) -> Result<sluice_seams::Running> {
        self.inner.running()
    }

    fn proven(
        &self,
        run: &RunHandle,
        report: &sluice_seams::RunReport,
    ) -> Result<sluice_seams::Proof> {
        self.inner.proven(run, report)
    }

    fn session_runs(&self) -> Result<Vec<sluice_seams::HostRun>> {
        self.inner.session_runs()
    }

    fn logs(&self, run: &RunHandle, tasks: &[String]) -> Result<Vec<TaskLog>> {
        self.inner.logs(run, tasks)
    }

    fn tasks(&self, run: &RunHandle) -> Result<Vec<TaskProgress>> {
        self.inner.tasks(run)
    }
}

/// The branch, with every move it made counted.
struct RecordedRefs {
    inner: Branches,
    moves: RefCell<usize>,
}

impl Branch for RecordedRefs {
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        self.inner.read(branch)
    }

    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        self.inner.held(branch)
    }

    fn copy(&self, branch: &str) -> Result<HostCopy> {
        self.inner.copy(branch)
    }

    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        self.inner.level(branch, expected, to)
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read_file(rev, path)
    }

    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        self.inner.form(onto, head)
    }

    fn commit(&self, commit: &LandingCommit) -> Result<Rev> {
        self.inner.commit(commit)
    }

    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool> {
        self.inner.contains(tip, rev)
    }

    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        self.inner.landed_by(tip, rev)
    }

    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        let moved = self.inner.move_to(branch, expected, to)?;
        if moved == Moved::Moved {
            *self.moves.borrow_mut() += 1;
        }
        Ok(moved)
    }

    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        self.inner.make(branch, at)
    }

    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        self.inner.remove(branch, expected)
    }
}

/// The driver, with each session's agent named by the rig, and every
/// outcome the round asked it to hand over kept, whether its carrier
/// answered `Handed`, which the core records as a hand-over by its way,
/// or `Undelivered`, as the real driver answers a session in no pane (51;
/// record 0102).
struct Sessions {
    inner: Drivers,
    agents: RefCell<BTreeMap<String, String>>,
    asked: RefCell<Vec<Notification>>,
}

impl Driver for Sessions {
    fn session(&self, worktree: &str) -> Result<Session> {
        let mut s = self.inner.session(worktree)?;
        if let Some(agent) = self.agents.borrow().get(worktree) {
            s.from.agent = agent.clone();
        }
        // The rig's sessions are in no pane: run in-process, the driver
        // would name the one this test runs in (51).
        s.from.session = None;
        Ok(s)
    }

    /// 124: the real driver's submitter of a branch by name, its agent
    /// the one the rig names for the branch.
    fn named(&self, branch: &str) -> Result<Submitter> {
        let mut from = self.inner.named(branch)?;
        if let Some(agent) = self.agents.borrow().get(branch) {
            from.agent = agent.clone();
        }
        Ok(from)
    }

    fn notify(&self, n: &Notification) -> Result<Delivery> {
        let delivery = self.inner.notify(n)?;
        // The rig's sessions are in no pane, so the real driver hands
        // nothing over: an answer that did is the core's to record by its
        // way, `notice`, and confirms nothing (D5).
        if let Delivery::Handed { confirms, .. } = &delivery {
            assert!(!confirms, "a notice confirms nothing: {delivery:?}");
        }
        self.asked.borrow_mut().push(n.clone());
        Ok(delivery)
    }
}

struct Real {
    repo: Repo,
    sessions: Scratch,
    /// The host's blocks of ports, kept with the rig: a sandboxed run
    /// writes nothing of the user's own state.
    _blocks: Scratch,
    /// The host's session runs, the rig's own, so another agent's runs on
    /// this host never shrink the bound it tests under (71).
    _host: Scratch,
    _running: Scratch,
    rules: PathBuf,
    /// While it exists, a release command written to test for its path,
    /// `RELEASE_OK` in the policy, succeeds; without it, it exits 2 (123).
    release_ok: PathBuf,
    checkout: RecordedPlaces,
    tests: RecordedChecks,
    branch: RecordedRefs,
    driver: Sessions,
    state: Records,
    /// The shared remote, for a rig whose records are on it.
    origin: Option<Scratch>,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = support::git_command()
        .current_dir(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@localhost")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@localhost")
        .stdin(Stdio::null())
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A policy written for the fakes, with moon in place of `fake`.
fn real_policy(policy: &str) -> String {
    policy.replace("runner = \"fake\"", "runner = \"moon\"")
}

impl Real {
    /// `bolt/x` holding the three projects under `policy`, its runner moon.
    fn new(name: &str, policy: &str) -> Real {
        Real::open(name, policy, false)
    }

    /// As [`Real::new`], `bolt/x` on a bare `origin` and its records there,
    /// under a policy naming this host's signer `box` and trusting it
    /// alone; this host signs with its own key (C.6; 78).
    fn peers(name: &str, policy: &str) -> Real {
        Real::open(name, policy, true)
    }

    fn open(name: &str, policy: &str, peers: bool) -> Real {
        let repo = Repo::new(name);
        let key = ssh_key::PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519)
            .unwrap();
        let policy = if peers {
            let line = format!("box {}", key.public_key().to_openssh().unwrap());
            policy.replace(
                "trust = [\"open\", \"sandbox\", \"container\"]\n",
                "trust = [\"box\"]\nremote = \"origin\"\n",
            ) + &format!("\n[signers.box]\nssh = \"{line}\"\nconfined = true\n")
        } else {
            policy.to_string()
        };
        let policy = policy.as_str();
        let rules = repo.dir().join(".git/rig-rules");
        std::fs::write(&rules, "").unwrap();
        let release_ok = repo.dir().join(".git/rig-release-ok");
        std::fs::write(&release_ok, "").unwrap();
        repo.write(
            ".config/sluice.toml",
            &real_policy(policy).replace("RELEASE_OK", &release_ok.display().to_string()),
        );
        repo.write(
            ".moon/workspace.yml",
            "projects:\n  a: 'a'\n  b: 'b'\n  c: 'c'\n\
             vcs:\n  client: 'git'\n  defaultBranch: 'main'\ntelemetry: false\n",
        );
        repo.write(
            "check.sh",
            &CHECK.replace("RULES", &rules.display().to_string()),
        );
        repo.write(".gitignore", ".moon/cache/\n");
        for p in ["a", "b", "c"] {
            repo.write(&format!("{p}/lib"), &format!("{p}1"));
            repo.write(
                &format!("{p}/moon.yml"),
                &format!(
                    "tasks:\n  test:\n    command: 'sh check.sh {p}:test'\n    inputs: ['**/*']\n    \
                     tags: [gate]\n    options: {{runFromWorkspaceRoot: true}}\n"
                ),
            );
        }
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "three projects"]);
        repo.git(&["branch", BRANCH]);
        let origin = peers.then(|| repo.bare_origin());
        let signing = peers.then(|| {
            let path = repo.dir().join(".git/rig-signing");
            key.write_openssh_file(&path, ssh_key::LineEnding::LF)
                .unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            sluice::instance::Signing {
                signer: "box".into(),
                key: path,
            }
        });

        // 35, design D7: the host shares moon's cache among its worktrees,
        // in the base checkout's `.moon/cache`: what lets a combination's
        // place replay what a session's run, or a passed combination, ran
        // (93).
        let moon_cache = HostCache {
            dir: repo.dir().join(".moon/cache"),
            socket: None,
            env: [
                ("MOON_CACHE_SHARED_WORKTREE_CACHE", "true"),
                ("MOON_EXPERIMENT_CAS_OUTPUTS_CACHE", "true"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        };
        let bound = Bound::open_with(
            repo.dir(),
            PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
            Instance {
                bound: 4,
                reserved: 0,
                processors: 4,
                forge: None,
                signing,
                peers: Default::default(),
                systems: None,
                caches: vec![moon_cache],
                expire_after: sluice_core::EXPIRE_AFTER,
                telemetry: None,
            },
            std::rc::Rc::new(sluice::retry::Policy::command().recording()),
        )
        .expect("the profiles bind");
        let Bound {
            checkout,
            tests,
            branch,
            driver,
            state,
            ..
        } = bound;
        let blocks = Scratch::new(&format!("{name}-blocks"));
        let host = Scratch::new(&format!("{name}-host-sessions"));
        let running = Scratch::new(&format!("{name}-host-running"));
        Real {
            sessions: Scratch::new(&format!("{name}-sessions")),
            repo,
            rules,
            release_ok,
            checkout: RecordedPlaces {
                inner: checkout
                    .with_ports(blocks.path().to_path_buf())
                    .with_scratch_root(support::test_user_scratch()),
                log: RefCell::default(),
            },
            tests: RecordedChecks {
                inner: tests
                    .with_sessions(host.path().to_path_buf())
                    .with_running(running.path().to_path_buf()),
                requests: RefCell::default(),
                started: RefCell::default(),
            },
            branch: RecordedRefs {
                inner: branch,
                moves: RefCell::default(),
            },
            driver: Sessions {
                inner: driver,
                agents: RefCell::default(),
                asked: RefCell::default(),
            },
            state,
            _blocks: blocks,
            _host: host,
            _running: running,
            origin,
        }
    }

    /// `task` runs past any time limit on a combination whose tree holds
    /// `path`, and as before on any other.
    fn hangs_with(&self, task: &str, path: &str) {
        self.fails_with(task, path, "-");
    }

    /// Until `run` has finished, with no round reading it.
    fn finished(&self, run: &RunHandle) {
        let until = Instant::now() + Duration::from_secs(300);
        while matches!(self.tests.progress(run).unwrap(), Progress::Running) {
            assert!(Instant::now() < until, "{} never ended", run.0);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Commit `changes` in the place `dir`; returns its head.
    fn commit(&self, dir: &Path, changes: &[(&str, Option<&str>)]) -> Rev {
        for (path, content) in changes {
            match content {
                Some(c) => {
                    let p = dir.join(path);
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                    std::fs::write(p, c).unwrap();
                }
                None => {
                    git(dir, &["rm", "-q", path]);
                }
            }
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "work"]);
        Rev::new(git(dir, &["rev-parse", "HEAD"]).trim())
    }
}

impl Rig for Real {
    fn kept_log(&self, at: &str) -> Option<String> {
        std::fs::read_to_string(at).ok()
    }

    fn sluice(&self) -> Sluice<'_> {
        Sluice::new(
            Seams {
                checkout: &self.checkout,
                tests: &self.tests,
                branch: &self.branch,
                driver: &self.driver,
                state: &self.state,
            },
            &SystemClock,
        )
        .as_peer(sluice_core::Peer {
            signer: self.origin.as_ref().map(|_| "box".to_string()),
            ..sluice_core::Peer::default()
        })
    }

    /// One round, taken again while the branch's lock is busy, as `sluice
    /// run` and a waiting submit do: tests share one process, and a child
    /// another test is starting holds a copy of every open file, the lock
    /// included, until it runs its program.
    fn step(&self) -> Step {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let step = self.sluice().step(BRANCH).expect("a step runs");
            if step != Step::Busy || Instant::now() >= until {
                return step;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn work(&self, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) -> Rev {
        let place = self.place(worktree);
        let dir = Path::new(&place);
        let tip = self.tip();
        if dir.exists() {
            git(dir, &["switch", "-q", "-C", worktree, tip.as_str()]);
        } else {
            git(
                self.repo.dir(),
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    worktree,
                    &place,
                    tip.as_str(),
                ],
            );
        }
        self.driver
            .agents
            .borrow_mut()
            .insert(place.clone(), agent.into());
        self.commit(dir, changes)
    }

    fn place(&self, worktree: &str) -> String {
        self.sessions
            .path()
            .join(worktree.replace('/', "-"))
            .display()
            .to_string()
    }

    fn work_on(&self, worktree: &str, on: &Rev, changes: &[(&str, Option<&str>)]) -> Rev {
        let place = self.place(worktree);
        let dir = Path::new(&place);
        git(dir, &["switch", "-q", "-C", worktree, on.as_str()]);
        self.commit(dir, changes)
    }

    fn branch_at(&self, name: &str, at: &Rev) {
        git(
            self.repo.dir(),
            &["update-ref", &format!("refs/heads/{name}"), at.as_str()],
        );
    }

    fn dirty(&self, worktree: &str, paths: &[&str]) {
        let place = self.place(worktree);
        for path in paths {
            let p = Path::new(&place).join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "beyond the commits").unwrap();
        }
    }

    fn remove_worktree(&self, worktree: &str) {
        git(
            self.repo.dir(),
            &["worktree", "remove", "--force", &self.place(worktree)],
        );
    }

    fn policy_text(&self, policy: &str) -> String {
        real_policy(policy)
    }

    fn release_fails(&self, fails: bool) {
        match fails {
            true => std::fs::remove_file(&self.release_ok).unwrap(),
            false => std::fs::write(&self.release_ok, "").unwrap(),
        }
    }

    fn releases_of(&self, name: &str) -> usize {
        let log = self.checkout.log.borrow();
        log.asked.iter().filter(|p| *p == name).count()
    }

    fn gone_releases_of(&self, name: &str) -> usize {
        let log = self.checkout.log.borrow();
        log.gone.iter().filter(|p| *p == name).count()
    }

    fn host(&self) -> String {
        sluice::profiles::checks::hostname()
    }

    fn fails_with(&self, task: &str, path: &str, content: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.rules)
            .unwrap();
        writeln!(f, "{task}\t{path}\t{content}").unwrap();
    }

    fn hangs(&self, task: &str) {
        self.fails_with(task, "-", "hangs");
    }

    /// Step until no submission is queued, under test or landing, and
    /// every place made is released: runs finish on their own here.
    fn settle(&self) {
        let until = Instant::now() + Duration::from_secs(300);
        loop {
            self.step();
            let status = self.sluice().status(BRANCH).expect("status reads");
            let busy = status.submissions.iter().any(|s| {
                matches!(
                    s.state,
                    Entry::Queued | Entry::Testing | Entry::Passed | Entry::Landing
                )
            });
            if !busy && self.live_places().is_empty() {
                return;
            }
            assert!(Instant::now() < until, "{BRANCH} never settled");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn delivered_to(&self, agent: &str) -> Vec<Notification> {
        self.driver
            .asked
            .borrow()
            .iter()
            .filter(|n| n.to.agent == agent)
            .cloned()
            .collect()
    }

    fn started(&self) -> Vec<Rev> {
        self.tests.started.borrow().clone()
    }

    fn requests(&self) -> Vec<RunRequest> {
        self.tests.requests.borrow().clone()
    }

    fn moves(&self) -> usize {
        *self.branch.moves.borrow()
    }

    fn on_remote(&self) -> bool {
        self.origin.is_some()
    }

    /// Another clone's push to the remote, or, for a branch in the host's
    /// own refs, a commit on it there.
    fn elsewhere(&self, changes: &[(&str, Option<&str>)]) -> Rev {
        let Some(origin) = &self.origin else {
            return Rev::new(self.repo.branch(BRANCH, BRANCH, changes));
        };
        let other = Repo::clone_of(origin.path(), "elsewhere");
        other.git(&["switch", "-q", BRANCH]);
        for (path, content) in changes {
            match content {
                Some(c) => other.write(path, c),
                None => {
                    other.git(&["rm", "-q", path]);
                }
            }
        }
        other.git(&["add", "-A"]);
        other.git(&["commit", "-q", "-m", "elsewhere"]);
        other.git(&["push", "-q", "origin", &format!("HEAD:{BRANCH}")]);
        Rev::new(other.rev("HEAD"))
    }

    fn tree(&self, rev: &Rev) -> String {
        git(
            self.repo.dir(),
            &["rev-parse", &format!("{}^{{tree}}", rev.as_str())],
        )
    }

    fn made_places(&self) -> Vec<String> {
        self.checkout.log.borrow().made.clone()
    }

    fn live_places(&self) -> Vec<String> {
        let log = self.checkout.log.borrow();
        log.made
            .iter()
            .filter(|n| !log.released.contains(n))
            .cloned()
            .collect()
    }

    fn parents(&self, rev: &Rev) -> Vec<Rev> {
        git(
            self.repo.dir(),
            &["rev-list", "--parents", "-n", "1", rev.as_str()],
        )
        .split_whitespace()
        .skip(1)
        .map(Rev::new)
        .collect()
    }

    /// The message exactly as the commit holds it.
    fn message(&self, rev: &Rev) -> String {
        let raw = git(self.repo.dir(), &["cat-file", "commit", rev.as_str()]);
        raw.split_once("\n\n").map_or("", |(_, m)| m).to_string()
    }
}

/// Each scenario of `shared` on its own rig: the bolt policy, or the one
/// the scenario's fake test gives it.
macro_rules! scenarios {
    ($($name:ident: $policy:expr;)*) => {$(
        #[test]
        fn $name() {
            shared::$name(&Real::new(stringify!($name), &$policy));
        }
    )*};
}

fn bolt() -> String {
    policy(false, "fail", 4)
}

fn batch() -> String {
    policy(true, "fail", 4)
}

/// The bolt policy with ports and a preview whose command does nothing.
fn preview() -> String {
    sluice_conformance::previewed(&bolt(), "true")
}

fn narrow() -> String {
    bolt().replace("[\"#gate\"]", "[\"a:test\"]")
}

/// The bolt policy with a release command that exits 2, printing what a
/// failing release prints, while the rig's `RELEASE_OK` is gone (123).
fn releasing() -> String {
    bolt().replace(
        "[tests]\n",
        &format!(
            "[tests]\nrelease = \"test -e RELEASE_OK || {{ echo {}; exit 2; }}\"\n",
            shared::RELEASE_FAILS
        ),
    )
}

/// [`releasing`], trying a failed release once more before it is held.
fn releasing_once() -> String {
    releasing().replace(
        "timeout = \"20m\"\n",
        "timeout = \"20m\"\nrelease_retries = 1\n",
    )
}

scenarios! {
    a_branch_is_held_as_the_host_last_read_it: bolt();
    records_held_after_a_refresh_are_the_records_read: bolt();
    s1_two_heads_pass_and_land_in_order: bolt();
    s2_a_conflict_is_told_before_any_check_runs: bolt();
    s3_the_second_fails_and_only_its_session_hears: bolt();
    s4_the_first_fails_and_the_second_is_formed_again_without_it: bolt();
    s9_deepest_first_finds_the_culprit_by_testing_what_it_skipped: batch();
    s11_a_landing_names_its_verdict_and_leaves_the_session_place: bolt();
    s12_a_submission_cannot_loosen_the_gate_it_is_judged_by: bolt();
    s16_a_forecast_names_the_file_and_the_work_and_writes_nothing: bolt();
    a_branch_is_forecast_with_no_place_of_it: bolt();
    every_submission_is_tested_on_top_of_every_one_ahead: bolt();
    state_and_status_read_the_records_with_no_step_running: bolt();
    a_resubmission_withdraws_the_earlier_submission_which_stays_readable: bolt();
    work_another_landing_took_is_landed_as_the_commit_that_landed_it: bolt();
    a_submission_waits_for_its_after_and_holds_no_one: bolt();
    a_submission_names_its_after_priority_and_submitter_and_does_nothing_more: bolt();
    a_place_holding_more_than_its_commits_is_refused_and_named: bolt();
    every_run_is_recorded_before_its_verdict_a_fail_exactly_as_a_pass: bolt();
    every_observed_result_records_what_its_executor_enforced: bolt();
    a_landed_change_to_the_gate_runs_every_check_it_now_requires: narrow();
    deepest_first_the_longest_passes_and_all_land_on_its_verdict: batch();
    a_conflict_names_every_submission_it_is_contested_with: bolt();
    every_record_written_is_the_real_format_and_names_its_schema: bolt();
    s26_a_clean_session_run_is_replayed_by_its_submission: bolt();
    s28_a_dirty_session_run_records_nothing: bolt();
    s27_a_sessions_place_stays_until_it_is_released: bolt();
    s30_a_landed_revision_is_previewed_at_its_addresses: preview();
    s31_the_next_landings_preview_replaces_the_last: preview();
    s32_asking_again_gives_back_the_same_preview: preview();
    a_removed_worktrees_place_is_freed_by_the_next_round: releasing();
    a_removed_worktrees_failing_release_is_freed_and_never_run_again: releasing();
    a_failed_release_is_held_with_what_failed: releasing();
    a_failed_release_is_tried_a_bounded_number_of_times: releasing();
    a_failed_release_is_held_after_the_retries_its_branch_names: releasing_once();
    a_release_that_succeeds_on_a_retry_is_listed_no_more: releasing();
    a_held_checkout_never_holds_its_branch: releasing();
    a_session_runs_last_result_says_whether_it_was_recorded: bolt();
    a_last_result_decides_nothing: bolt();
    a_worktree_that_changes_while_its_run_runs_records_nothing: bolt();
    a_withdrawn_combinations_run_is_ended_and_its_checkout_released: bolt();
    a_withdrawn_combinations_passed_run_is_read_for_nothing: bolt();
    a_withdrawn_combinations_failed_run_blames_no_one: bolt();
    a_withdrawn_combinations_failed_release_holds_its_checkout: releasing();
    a_quiet_branch_mended_outside_sluice_is_read_whole: bolt();
    status_of_a_submission_on_a_mended_branch_answers_for_it: bolt();
    a_refusal_names_the_revision_this_host_holds: bolt();
    a_read_of_a_mended_branch_leaves_its_state_as_it_was: bolt();
}

/// Each scenario of `shared` with the branch's records on its remote and
/// every result signed by this host (C.6; 78, 109; ADR 0037).
macro_rules! peers_scenarios {
    ($($test:ident => $name:ident: $policy:expr;)*) => {$(
        #[test]
        fn $test() {
            shared::$name(&Real::peers(stringify!($test), &$policy));
        }
    )*};
}

peers_scenarios! {
    peers_held => a_branch_is_held_as_the_host_last_read_it: bolt();
    peers_records_held => records_held_after_a_refresh_are_the_records_read: bolt();
    peers_s1 => s1_two_heads_pass_and_land_in_order: bolt();
    peers_s3 => s3_the_second_fails_and_only_its_session_hears: bolt();
    peers_s4 => s4_the_first_fails_and_the_second_is_formed_again_without_it: bolt();
    peers_s9 => s9_deepest_first_finds_the_culprit_by_testing_what_it_skipped: batch();
    peers_recorded => every_run_is_recorded_before_its_verdict_a_fail_exactly_as_a_pass: bolt();
    peers_state_read => state_and_status_read_the_records_with_no_step_running: bolt();
}

/// The refs of the rig's shared remote.
fn origin_refs(r: &Real) -> Vec<String> {
    let bare = r.origin.as_ref().expect("a rig with records on its remote");
    String::from_utf8(
        support::git_command()
            .arg("--git-dir")
            .arg(bare.path())
            .args(["for-each-ref", "--format=%(refname)"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .lines()
    .map(str::to_string)
    .collect()
}

/// The remote-branch spec's "State on the remote stays under
/// refs/sluice": sluice changes nothing on the remote but the branch and
/// what is under `refs/sluice/`.
#[test]
fn peers_push_only_the_branch_and_refs_sluice() {
    let r = Real::peers("peers-refs", &bolt());
    let before: Vec<String> = origin_refs(&r)
        .into_iter()
        .filter(|n| !n.starts_with("refs/sluice/"))
        .collect();
    shared::s1_two_heads_pass_and_land_in_order(&r);
    let after = origin_refs(&r);
    assert!(after
        .iter()
        .any(|n| n == &format!("refs/sluice/state/{BRANCH}")));
    assert!(after.iter().any(|n| n == "refs/sluice/runs/box"));
    for n in &after {
        assert!(
            n.starts_with("refs/sluice/") || before.contains(n),
            "sluice pushed {n}"
        );
    }
}

/// 80: with the remote out of reach, the state reads as last fetched,
/// and the status says so.
#[test]
fn peers_read_the_last_fetched_state_when_the_remote_is_unreachable() {
    let r = Real::peers("peers-stale", &bolt());
    shared::s1_two_heads_pass_and_land_in_order(&r);
    let s = r.sluice();
    let landed = s.status(BRANCH).unwrap().submissions.len();
    assert_eq!(s.status(BRANCH).unwrap().stale, None);
    let bare = r.origin.as_ref().unwrap().path();
    let away = bare.with_extension("away");
    std::fs::rename(bare, &away).unwrap();
    let status = r.sluice().status(BRANCH).unwrap();
    std::fs::rename(&away, bare).unwrap();
    assert_eq!(status.stale.as_deref(), Some("origin"));
    assert_eq!(status.submissions.len(), landed);
}

/// Rule core.14; 25, 34 (swb-1, 2026-11-12: `sluice wait 56` exiting 3,
/// every round refused at `keeping <branch>'s tasks without holding its
/// lock`; breadboard-kit's tries on another host left with their places
/// and their containers' ports, no `released` logged): with the branch's
/// records on its remote, the round after one of the host's runs ended,
/// while another of its runs still tests and says what its tasks are,
/// keeps those tasks under the lock it took, writes its records, lands
/// what passed and releases the ended run's try in that round.
#[test]
fn peers_a_finished_try_is_released_while_another_run_tests() {
    use sluice_core::records::RunState;

    let r = Real::peers("peers-tasks", &bolt());
    r.hangs_with("a:test", "a/hangs");
    r.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    r.work("unit/a", "agent-a", &[("a/hangs", Some("yes"))]);
    let b = r.submit("unit/b");
    let a = r.submit("unit/a");
    assert_eq!(r.step(), Step::Stepped);
    assert_eq!(r.tested(), [vec![b], vec![b, a]]);
    let run_of = |ids: &[u64]| {
        let doc = r.doc();
        let c = doc
            .candidates
            .iter()
            .find(|c| c.submissions == ids)
            .expect("the combination is formed");
        let run = &c.runs[0];
        (
            c.id.clone(),
            run.place.clone().expect("a place"),
            run.run.clone().expect("a run started"),
        )
    };
    let (_, first, ends) = run_of(&[b]);
    let (both, second, tests) = run_of(&[b, a]);
    r.finished(&ends);
    let until = Instant::now() + Duration::from_secs(300);
    while r.tests.tasks(&tests).unwrap().is_empty() {
        assert!(Instant::now() < until, "the run never said its tasks");
        std::thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(r.step(), Step::Stepped);
    assert_eq!(r.sub(b).state, Entry::Landed);
    assert!(!r.live_places().contains(&first), "{first} is left");
    assert_eq!(r.releases_of(&first), 1);
    assert!(r.live_places().contains(&second));
    let doc = r.doc();
    let still = doc.candidates.iter().find(|c| c.id == both).unwrap();
    assert_eq!(still.runs[0].state, RunState::Testing);
    let kept = sluice_seams::State::progress(&r.state, BRANCH).unwrap();
    assert!(
        kept.get(&both).is_some_and(|t| !t.is_empty()),
        "{both}'s tasks are not kept: {kept:?}"
    );

    r.withdraw(a, sluice_core::Requester::Session("agent-a".into()));
    r.step();
    assert!(r.live_places().is_empty(), "{:?}", r.live_places());
}

/// The run-outcome spec's "The step that started it has exited" (66; rule
/// core.10): a step under a one-second limit starts a run whose check
/// sleeps for minutes, and returns while it goes on; with no step running,
/// the run's supervisor ends its process group within the grace after the
/// limit and writes how the run ended, and the step after that records
/// the run timed out.
#[test]
fn a_run_is_ended_at_its_limit_after_the_step_that_started_it() {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;
    use sluice_core::records::Event;

    let policy = bolt().replace("timeout = \"20m\"", "timeout = \"1s\"");
    let r = Real::new("limit-after-step", &policy);
    r.hangs("a:test");
    r.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    r.submit("unit/a");
    let started = Instant::now();
    assert_eq!(r.step(), Step::Stepped);
    let tested = r.doc().candidates[0].clone();
    let run = tested.runs[0].run.clone().expect("a run started");
    let dir = r.tests.inner.runs().dir(&run.0);
    let group = loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("pgid")) {
            break Pid::from_raw(text.trim().parse().unwrap());
        }
        assert!(started.elapsed() < Duration::from_secs(30), "never started");
        std::thread::sleep(Duration::from_millis(20));
    };
    // From the run's start, when its group was made: the limit, its grace,
    // and the wait after the kill that follows it.
    let ended_by = Instant::now() + Duration::from_secs(1 + 10 + 5);
    while killpg(group, None) != Err(Errno::ESRCH) {
        assert!(
            Instant::now() < ended_by,
            "the run outlived its limit by its grace"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // The group gone is not yet the run's end: the supervisor writes how
    // the run ended after it, the last of its ending, and a step before
    // that reads the run still going and records nothing.
    while sluice::runs::ended(&dir).unwrap().is_none() {
        assert!(
            Instant::now() < ended_by,
            "the run's group is gone, and its supervisor wrote no end within its limit and grace"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(r.step(), Step::Stepped);
    let events = r.events();
    let timed_out = events.iter().any(|e| {
        matches!(e, Event::Result { combination, result, .. }
            if *combination == tested.revision && result == "timed out")
    });
    assert!(
        timed_out,
        "the step after the run's end recorded no timed-out result: {events:?}"
    );
}
