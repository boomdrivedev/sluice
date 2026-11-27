//! `sluice exec` on a scratch repository (session-runs-1 clauses 89 to
//! 92): the binary, a real place and a real supervisor. Needs git, so it
//! is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use support::{Repo, Scratch};

const POLICY: &str = "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
[checkout]\nports = [\"web\"]\nports_from = \"41500\"\n\n\
[branches.\"bolt/*\"]\ntimeout = \"1m\"\n";

struct Setup {
    repo: Repo,
    worktree: PathBuf,
    config: Scratch,
    state: Scratch,
    _sessions: Scratch,
}

impl Setup {
    fn new(name: &str) -> Setup {
        Setup::at(name, "a")
    }

    /// A worktree at `rel` under the scratch directory for sessions.
    fn at(name: &str, rel: &str) -> Setup {
        let repo = Repo::new(name);
        repo.write(".config/sluice.toml", POLICY);
        repo.write("check.sh", "echo checked >> \"$SLUICE_TMP/checked\"\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        repo.git(&["branch", "bolt/x", "main"]);
        let sessions = Scratch::new(&format!("{name}-sessions"));
        let worktree = sessions.path().join(rel);
        repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "unit/a",
            worktree.to_str().unwrap(),
            "bolt/x",
        ]);
        std::fs::write(worktree.join("a.txt"), "a").unwrap();
        let git = |args: &[&str]| {
            let mut a = vec!["-C", worktree.to_str().unwrap()];
            a.extend(args);
            repo.git(&a)
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "a"]);
        Setup {
            repo,
            worktree,
            config: Scratch::new(&format!("{name}-config")),
            state: Scratch::new(&format!("{name}-state")),
            _sessions: sessions,
        }
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.sluice_with(&[], args)
    }

    /// [`Setup::sluice`], with `env` set beside the rest.
    fn sluice_with(&self, env: &[(&str, &Path)], args: &[&str]) -> Output {
        self.command(args)
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    /// The `sluice` that [`Setup::sluice`] runs, not yet started.
    fn command(&self, args: &[&str]) -> Command {
        self.prepared(Command::new(env!("CARGO_BIN_EXE_sluice")), args)
    }

    /// `c`, a command that runs `sluice` given `args`, in this setup's
    /// worktree on its configuration and state, not yet started.
    fn prepared(&self, mut c: Command, args: &[&str]) -> Command {
        support::isolated(&mut c)
            .args(args)
            .current_dir(&self.worktree)
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        c
    }

    /// `sluice` started in this setup's worktree on `host`'s host, its
    /// configuration and state, as another repository on that host runs
    /// it; its stdout piped, its stderr the test's.
    fn spawn_on(&self, host: &Setup, args: &[&str]) -> Child {
        support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
            .args(args)
            .current_dir(&self.worktree)
            .env("XDG_CONFIG_HOME", host.config.path())
            .env("XDG_STATE_HOME", host.state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap()
    }

    /// Commit `policy` on `bolt/x`, as the branch's policy.
    fn policy(&self, policy: &str) {
        self.repo.write(".config/sluice.toml", policy);
        self.repo.git(&["commit", "-q", "-am", "the policy"]);
        self.repo.git(&["branch", "-f", "bolt/x", "main"]);
    }

    fn results(&self) -> String {
        std::fs::read_to_string(self.repo.dir().join(".git/sluice/results.jsonl"))
            .unwrap_or_default()
    }

    fn sessions(&self) -> Vec<String> {
        match std::fs::read_dir(self.state.path().join("sluice/sessions")) {
            Ok(d) => d
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Where the binary looks for the container engine in place of the
/// host's sockets (record 0074).
const ENGINE_SOCKETS: &str = "SLUICE_ENGINE_SOCKETS";

/// `bolt/x` holding a moon workspace of two projects whose `test` tasks,
/// tagged `gate`, the policy requires of runs that ask for the container
/// engine: `a`'s runs `a_check`, `b`'s passes. The session's head changes
/// both. Needs moon on the path.
fn moon_setup(name: &str, a_check: &str) -> Setup {
    let repo = Repo::new(name);
    repo.write(
        ".config/sluice.toml",
        "[tests]\nrunner = \"moon\"\n\n[executor]\ndocker = true\n\n\
         [branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntimeout = \"5m\"\n",
    );
    repo.write(
        ".moon/workspace.yml",
        "projects:\n  a: 'a'\n  b: 'b'\n\
         vcs:\n  client: 'git'\n  defaultBranch: 'main'\ntelemetry: false\n",
    );
    repo.write(".gitignore", ".moon/cache/\n");
    repo.write("a/check.sh", a_check);
    for (p, command) in [("a", "sh a/check.sh"), ("b", "true")] {
        repo.write(&format!("{p}/lib"), "1");
        repo.write(
            &format!("{p}/moon.yml"),
            &format!(
                "tasks:\n  test:\n    command: '{command}'\n    inputs: ['**/*']\n    \
                 tags: [gate]\n    options: {{runFromWorkspaceRoot: true}}\n"
            ),
        );
    }
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "two projects"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let sessions = Scratch::new(&format!("{name}-sessions"));
    let worktree = sessions.path().join("a");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/a",
        worktree.to_str().unwrap(),
        "bolt/x",
    ]);
    for p in ["a", "b"] {
        std::fs::write(worktree.join(p).join("lib"), "2").unwrap();
    }
    let git = |args: &[&str]| {
        let mut a = vec!["-C", worktree.to_str().unwrap()];
        a.extend(args);
        repo.git(&a)
    };
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "a and b"]);
    Setup {
        repo,
        worktree,
        config: Scratch::new(&format!("{name}-config")),
        state: Scratch::new(&format!("{name}-state")),
        _sessions: sessions,
    }
}

/// `bolt/x` holding a moon workspace of two projects whose `test` tasks,
/// tagged `gate`, the policy requires: `a`'s runs `a_check`, `b`'s passes.
/// The session's worktree holds exactly `bolt/x`'s revision, nothing of
/// its own. Needs moon on the path.
fn moon_at_branch(name: &str, a_check: &str) -> Setup {
    let repo = Repo::new(name);
    repo.write(
        ".config/sluice.toml",
        "[tests]\nrunner = \"moon\"\n\n\
         [branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntimeout = \"5m\"\n",
    );
    repo.write(
        ".moon/workspace.yml",
        "projects:\n  a: 'a'\n  b: 'b'\n\
         vcs:\n  client: 'git'\n  defaultBranch: 'main'\ntelemetry: false\n",
    );
    repo.write(".gitignore", ".moon/cache/\n");
    repo.write("a/check.sh", a_check);
    for (p, command) in [("a", "sh a/check.sh"), ("b", "true")] {
        repo.write(&format!("{p}/lib"), "1");
        repo.write(&format!("{p}/moon.yml"), &moon_yml(command));
    }
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "two projects"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let sessions = Scratch::new(&format!("{name}-sessions"));
    let worktree = sessions.path().join("a");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/a",
        worktree.to_str().unwrap(),
        "bolt/x",
    ]);
    Setup {
        repo,
        worktree,
        config: Scratch::new(&format!("{name}-config")),
        state: Scratch::new(&format!("{name}-state")),
        _sessions: sessions,
    }
}

/// A project's `moon.yml` of [`moon_at_branch`]: its `test` task runs
/// `command` from the workspace's root.
fn moon_yml(command: &str) -> String {
    format!(
        "tasks:\n  test:\n    command: '{command}'\n    inputs: ['**/*']\n    \
         tags: [gate]\n    options: {{runFromWorkspaceRoot: true}}\n"
    )
}

impl Setup {
    /// Write `files` in the worktree and commit them on its branch.
    fn commit(&self, files: &[(&str, &str)]) -> String {
        for (path, content) in files {
            std::fs::write(self.worktree.join(path), content).unwrap();
        }
        let dir = self.worktree.to_str().unwrap();
        self.repo.git(&["-C", dir, "add", "-A"]);
        self.repo
            .git(&["-C", dir, "commit", "-q", "-m", "the worktree's own"]);
        self.repo.git(&["-C", dir, "rev-parse", "HEAD"])
    }

    /// Land `files` on `bolt/x`, as the branch moving on past the worktree.
    fn land(&self, files: &[(&str, &str)]) -> String {
        for (path, content) in files {
            self.repo.write(path, content);
        }
        self.repo.git(&["add", "-A"]);
        self.repo.git(&["commit", "-q", "-m", "landed"]);
        self.repo.git(&["branch", "-f", "bolt/x", "main"]);
        self.repo.rev("bolt/x")
    }
}

/// The `--json` document of `sluice exec`.
fn document(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(out)))
}

/// The checks a `sluice exec --json` ran or replayed, by name, sorted.
fn checks_of(out: &Output) -> Vec<String> {
    let doc = document(out);
    let mut names: Vec<String> = doc["tasks"]
        .as_array()
        .unwrap_or_else(|| panic!("no tasks in {doc}"))
        .iter()
        .map(|t| t["task"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

/// The exec-checks spec's "The branch moved on" (36; design D1): a
/// worktree's own changes are read from where its head left the branch,
/// so what the branch landed since is none of its own.
#[test]
fn a_worktrees_checks_leave_out_what_its_branch_landed_since() {
    let s = moon_at_branch("exec-moved-on", "true\n");
    s.commit(&[("b/lib", "2")]);
    s.land(&[
        ("a/lib", "2"),
        ("a/moon.yml", &moon_yml("sh a/check.sh # moved")),
    ]);
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(checks_of(&out), ["b:test"], "{}", text(&out));
}

/// The exec-checks spec's "An edit not committed" (36; design D2): an
/// unstaged edit is the worktree's own, and its run records nothing (92).
#[test]
fn a_worktrees_unstaged_edit_runs_the_checks_it_affects() {
    let s = moon_at_branch("exec-unstaged", "true\n");
    std::fs::write(s.worktree.join("a/lib"), "2").unwrap();
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(checks_of(&out), ["a:test"], "{}", text(&out));
    assert!(
        text(&out).contains("not recorded: worktree is not clean"),
        "{}",
        text(&out)
    );
    assert_eq!(s.results(), "", "nothing is recorded");
}

/// The exec-checks spec's "A file not yet tracked" (36; design D2): an
/// untracked file git does not ignore is the worktree's own.
#[test]
fn a_worktrees_untracked_file_runs_the_checks_it_affects() {
    let s = moon_at_branch("exec-untracked", "true\n");
    std::fs::write(s.worktree.join("b/new"), "new").unwrap();
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(checks_of(&out), ["b:test"], "{}", text(&out));
}

/// The exec-checks spec's "Nothing of its own" (36): a worktree that
/// holds exactly its branch's revision runs no check, and exits 0.
#[test]
fn a_worktree_at_its_branchs_revision_runs_no_check() {
    let s = moon_at_branch("exec-nothing", "exit 1\n");
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(checks_of(&out).is_empty(), "{}", text(&out));
}

/// The exec-checks spec's "Its own moon.yml" (36): a worktree's own
/// change to a project's `moon.yml` runs that project's checks alone
/// (the change `the-gate-runs-in-five-minutes`, design D1).
#[test]
fn a_worktrees_own_moon_yml_runs_its_project() {
    let s = moon_at_branch("exec-own-moon-yml", "true\n");
    s.commit(&[("b/moon.yml", &moon_yml("true # its own"))]);
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(checks_of(&out), ["b:test"], "{}", text(&out));
}

/// The exec-checks spec's "The toolchain" (36, 140): a worktree's own
/// change to a file the toolchain is set up from runs every required
/// check.
#[test]
fn a_worktrees_toolchain_runs_every_check() {
    let s = moon_at_branch("exec-own-toolchain", "true\n");
    s.commit(&[("devenv.nix", "{ }\n")]);
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(checks_of(&out), ["a:test", "b:test"], "{}", text(&out));
}

/// The exec-checks spec's "A worktree behind its branch" and "No history
/// in common" (36, 70; design D6): the run opens with where the worktree
/// left its branch, the branch's revision where it moved on past it, and
/// says a head that shares no history with the branch runs every check.
#[test]
fn exec_opens_with_where_the_worktree_left_its_branch() {
    let s = moon_at_branch("exec-opens", "true\n");
    let cut = s.repo.rev("bolt/x");
    s.commit(&[("b/lib", "2")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let first = |out: &Output| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .map(String::from)
    };
    assert_eq!(
        first(&out).as_deref(),
        Some(
            format!(
                "running the checks the worktree's own changes affect since {} on bolt/x",
                &cut[..7]
            )
            .as_str()
        ),
        "{}",
        text(&out)
    );

    let now = s.land(&[("a/lib", "2")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        first(&out).as_deref(),
        Some(
            format!(
                "running the checks the worktree's own changes affect since {} on bolt/x, \
                 now at {}",
                &cut[..7],
                &now[..7]
            )
            .as_str()
        ),
        "{}",
        text(&out)
    );

    let dir = s.worktree.to_str().unwrap();
    s.repo
        .git(&["-C", dir, "switch", "-q", "--orphan", "unit/unrelated"]);
    s.repo
        .git(&["-C", dir, "checkout", "-q", "unit/a", "--", "."]);
    s.commit(&[]);
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        said.lines().any(|l| l
            == "running every required check: the worktree's head shares no history with bolt/x"),
        "{said}"
    );
    assert_eq!(checks_of(&out), ["a:test", "b:test"], "{}", text(&out));
}

/// The exec-checks spec's "The same files again" (16, 108; design D4): a
/// clean worktree's second run on the files its first passed and recorded
/// replays both checks, counts them, and records nothing more.
#[test]
fn a_second_run_on_the_same_files_replays_and_records_nothing() {
    let s = moon_at_branch("exec-same-files", "true\n");
    s.commit(&[("a/lib", "2"), ("b/lib", "2")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("passed: 2 tasks\n"), "{}", text(&out));
    let mut first = recorded(&s);
    first.sort();
    assert_eq!(
        first,
        [
            ("a:test".to_string(), "pass".to_string()),
            ("b:test".to_string(), "pass".to_string())
        ]
    );
    let results = s.results();

    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("passed: 2 tasks (2 replayed)"),
        "{}",
        text(&out)
    );
    assert_eq!(s.results(), results, "a replay is never recorded as a run");
}

/// The exec-checks spec's "After a fix" (108; design D4): once a clean
/// run recorded a pass of `b:test` and a fail of `a:test`, a commit to
/// `a/lib` alone runs `a:test` again and replays `b:test`.
#[test]
fn after_a_fix_exec_runs_the_fixed_check_and_replays_the_rest() {
    let s = moon_at_branch("exec-after-a-fix", "grep -qx ok a/lib\n");
    s.commit(&[("a/lib", "broken"), ("b/lib", "2")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let mut first = recorded(&s);
    first.sort();
    assert_eq!(
        first,
        [
            ("a:test".to_string(), "fail".to_string()),
            ("b:test".to_string(), "pass".to_string())
        ],
        "{}",
        text(&out)
    );

    s.commit(&[("a/lib", "ok\n")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("passed: 2 tasks (1 replayed)"),
        "{}",
        text(&out)
    );
    let then = recorded(&s);
    assert_eq!(
        then[first.len()..],
        [("a:test".to_string(), "pass".to_string())],
        "a:test ran, and b:test's replay is recorded as nothing: {}",
        text(&out)
    );
}

/// The exec-checks spec's "A pass sluice never recorded" (92, 108;
/// design D5): a check passed with its change unstaged records nothing, so
/// once the change is committed as it was, the next run's replay of it is
/// proven by nothing; exec says so, runs it again and records its pass.
#[test]
fn a_pass_sluice_never_recorded_runs_again_and_is_recorded() {
    let s = moon_at_branch("exec-never-recorded", "true\n");
    std::fs::write(s.worktree.join("a/lib"), "2").unwrap();
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("not recorded: worktree is not clean"),
        "{}",
        text(&out)
    );
    assert_eq!(s.results(), "", "nothing is recorded");

    let head = s.commit(&[]);
    let out = s.sluice(&["exec", "bolt/x"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("replayed with no result sluice recorded: a:test; running them again\n"),
        "{said}"
    );
    assert!(said.contains("passed: 1 task\n"), "{said}");
    assert!(
        said.contains(&format!(
            "recorded 1 result under its key for {}",
            &head[..7]
        )),
        "{said}"
    );
    assert_eq!(recorded(&s), [("a:test".to_string(), "pass".to_string())]);
}

/// The exec-checks spec's "The document for scripts" (design D6): `--json`
/// gives `since`, the merge base's full revision, and each task's
/// `replayed`.
#[test]
fn exec_json_gives_since_and_each_tasks_replayed() {
    let s = moon_at_branch("exec-json-replayed", "true\n");
    let cut = s.repo.rev("bolt/x");
    s.commit(&[("a/lib", "2"), ("b/lib", "2")]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));

    s.commit(&[("a/lib", "3")]);
    let out = s.sluice(&["exec", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc = document(&out);
    assert_eq!(doc["since"], cut.as_str(), "{doc}");
    let task = |name: &str| {
        doc["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["task"] == name)
            .unwrap_or_else(|| panic!("no {name} in {doc}"))
            .clone()
    };
    assert_eq!(task("b:test")["replayed"], true, "{doc}");
    assert_eq!(task("b:test")["recorded"], false, "{doc}");
    assert_eq!(task("a:test")["replayed"], false, "{doc}");
    assert_eq!(task("a:test")["recorded"], true, "{doc}");
}

/// Each result `sluice exec` recorded: `(task, outcome)`.
fn recorded(s: &Setup) -> Vec<(String, String)> {
    s.results()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["task"].as_str().unwrap().to_string(),
                v["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// The host's configuration in `config`: a bound of `max`.
fn host_bound(config: &Scratch, max: usize) {
    std::fs::create_dir_all(config.path().join("sluice")).unwrap();
    std::fs::write(
        config.path().join("sluice/config.toml"),
        format!("[runs]\nmax = {max}\n"),
    )
    .unwrap();
}

/// The host's configuration in `config`: one cache whose clients read
/// `SCCACHE_DIR`.
fn host_cache(config: &Scratch) {
    std::fs::create_dir_all(config.path().join("sluice")).unwrap();
    std::fs::write(
        config.path().join("sluice/config.toml"),
        format!(
            "[[caches]]\ndir = {:?}\nenv = {{ SCCACHE_DIR = \"/caches/sccache\" }}\n",
            config.path().join("cache").display().to_string()
        ),
    )
    .unwrap();
}

/// Wait until `done` holds, for at most a minute.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < until, "{what} never happened");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The lines `child` prints, as they come.
fn lines_of(child: &mut Child) -> std::sync::mpsc::Receiver<String> {
    let out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// 89, 90, 92: a command gets the worktree as its checkout, its scratch
/// directory and its port, the same port on its next run; its exit is the
/// command's, and nothing is recorded.
#[test]
fn a_command_gets_the_worktrees_environment_and_its_exit() {
    let s = Setup::new("exec-command");
    let seen = |n: &str| -> String {
        let out = s.sluice(&[
            "exec",
            "bolt/x",
            "--",
            "sh",
            "-c",
            &format!("echo \"$SLUICE_CHECKOUT $SLUICE_PORT_WEB $SLUICE_PLACE\" > seen-{n}; exit 7"),
        ]);
        assert_eq!(out.status.code(), Some(7), "{}", text(&out));
        std::fs::read_to_string(s.worktree.join(format!("seen-{n}"))).unwrap()
    };
    let one = seen("1");
    let two = seen("2");
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    let words: Vec<&str> = one.split_whitespace().collect();
    assert_eq!(words[0], canon.to_str().unwrap());
    assert!(words[1].parse::<u16>().unwrap() >= 41500, "{one}");
    assert!(words[2].starts_with("sluice-"), "{one}");
    assert_eq!(one, two, "the same place and port on the second run");
    assert_eq!(s.results(), "", "a command records nothing");
    assert!(s.sessions().is_empty(), "the run is no longer counted");
}

/// 135; record 0126: sluice started under a supervisor's low soft limit
/// on open files, launchd's 256, raises it to its hard limit, as far as
/// the system lets one process open, and the command it runs has the
/// raised limit.
#[test]
fn a_command_runs_under_the_raised_limit_on_open_files() {
    let s = Setup::new("exec-open-files");
    let mut launched = Command::new("sh");
    launched.args([
        "-c",
        "ulimit -S -n 256 && exec \"$@\"",
        "sh",
        env!("CARGO_BIN_EXE_sluice"),
    ]);
    let out = s
        .prepared(
            launched,
            &[
                "exec",
                "bolt/x",
                "--",
                "sh",
                "-c",
                "printf '%s %s %s' \"$(ulimit -S -n)\" \"$(ulimit -H -n)\" \
                 \"$(sysctl -n kern.maxfilesperproc 2>/dev/null)\" > seen",
            ],
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let seen = std::fs::read_to_string(s.worktree.join("seen")).unwrap();
    let words: Vec<&str> = seen.split_whitespace().collect();
    let limit = |w: &str| (w != "unlimited").then(|| w.parse::<u64>().unwrap());
    // The most one process may open: its hard limit, and on macOS no more
    // than kern.maxfilesperproc.
    let most = [limit(words[1]), words.get(2).and_then(|w| limit(w))]
        .into_iter()
        .flatten()
        .min();
    assert_eq!(limit(words[0]), most, "soft, hard, per process: {seen}");
    assert_ne!(limit(words[0]), Some(256), "{seen}");
}

/// 35, 89: an agent's run in its worktree is given the host's caches as
/// a submission's run is: the variables the host's config declares.
#[test]
fn sluice_exec_gets_the_hosts_cache_environment() {
    let s = Setup::new("exec-cache-env");
    host_cache(&s.config);
    let out = s.sluice(&["exec", "bolt/x", "--", "sh", "-c", "env > seen"]);
    assert!(out.status.success(), "{}", text(&out));
    let seen = std::fs::read_to_string(s.worktree.join("seen")).unwrap();
    assert!(
        seen.lines().any(|l| l == "SCCACHE_DIR=/caches/sccache"),
        "{seen}"
    );
}

/// 153 (design D1, D2, D8): the binary's `exec`, started with
/// `TRACEPARENT` `00-<t>-<s>-01` and given `--otel`, sends a `sluice exec` span beneath `s`,
/// and its run's checks get a `TRACEPARENT` naming trace `t` and a span
/// other than `s`, the exec's. The trace and span ids are bytes that read
/// as text, `ABCDEFGHIJKLMNOP` and `abcdefgh`, so the protobuf the
/// collector is sent holds them as written: a span's parent id, field 4,
/// is encoded just before its name, field 5.
#[test]
fn an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace() {
    let s = Setup::new("exec-caller-trace");
    let collector = support::http::Fake::start(|_| (200, Vec::new(), String::new()));
    let (trace, parent) = ("4142434445464748494a4b4c4d4e4f50", "6162636465666768");
    let traceparent = format!("00-{trace}-{parent}-01");
    // None of the host's `OTEL_*`, which a suite run under `sluice exec`
    // is handed, a signal its table turns off among them.
    let mut c = s.command(&["exec", "--otel", "bolt/x", "--", "sh", "-c", "env > seen"]);
    for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("OTEL_")) {
        c.env_remove(k);
    }
    let out = c
        .env("TRACEPARENT", &traceparent)
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", collector.url())
        .env("OTEL_METRICS_EXPORTER", "none")
        .env("OTEL_LOGS_EXPORTER", "none")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let seen = std::fs::read_to_string(s.worktree.join("seen")).unwrap();
    let handed = seen
        .lines()
        .find_map(|l| l.strip_prefix("TRACEPARENT="))
        .unwrap_or_else(|| panic!("no TRACEPARENT in {seen}: {}", text(&out)));
    let fields: Vec<&str> = handed.split('-').collect();
    assert_eq!(fields.len(), 4, "{handed}");
    assert_eq!(fields[1], trace, "{handed}");
    assert_ne!(fields[2], parent, "{handed}");

    let sent: Vec<String> = collector
        .requests()
        .into_iter()
        .filter(|r| r.path == "/v1/traces")
        .map(|r| r.body)
        .collect();
    assert!(
        sent.iter()
            .any(|b| b.contains("\"\u{8}abcdefgh*\u{b}sluice exec")),
        "no sluice exec span beneath the caller's in {sent:?}"
    );
    assert!(
        sent.iter().any(|b| b.contains("\n\u{10}ABCDEFGHIJKLMNOP")),
        "nothing in the caller's trace in {sent:?}"
    );
}

/// Every file name kept under `dir`, walked whole.
/// 64: before it runs, exec says whether the head would form if
/// submitted now; a conflict is told and the run goes on, and `--json`
/// carries the forecast.
#[test]
fn exec_tells_a_conflict_before_it_runs_and_runs_anyway() {
    let s = Setup::new("exec-forecast");
    let forecast = |out: &Output| -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["forecast"].clone()
    };
    let out = s.sluice(&["exec", "bolt/x", "--json", "--", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(forecast(&out)["conflict"], serde_json::Value::Null);
    assert!(!text(&out).contains("would conflict"), "{}", text(&out));

    // The branch moves on, changing what the head changed.
    s.repo.git(&["switch", "-q", "bolt/x"]);
    s.repo.write("a.txt", "b");
    s.repo.git(&["add", "-A"]);
    s.repo.git(&["commit", "-q", "-m", "b"]);
    s.repo.git(&["switch", "-q", "main"]);

    let out = s.sluice(&["exec", "bolt/x", "--json", "--", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        forecast(&out)["conflict"]["paths"],
        serde_json::json!(["a.txt"])
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("would conflict: a.txt with bolt/x"),
        "{}",
        text(&out)
    );
    let out = s.sluice(&["exec", "bolt/x", "--", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).starts_with("would conflict: a.txt with bolt/x\n"),
        "{}",
        text(&out)
    );
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries {
        let e = e.unwrap();
        out.push(e.file_name().to_string_lossy().into_owned());
        if e.file_type().unwrap().is_dir() {
            out.extend(file_names(&e.path()));
        }
    }
    out
}

/// 89, 26, rule core.11: a worktree whose path, written as one file name,
/// is longer than any file name may be runs its checks, is recorded and
/// is listed by its whole path, and every file name sluice keeps for it
/// fits.
#[test]
fn exec_runs_from_a_worktree_at_a_long_path() {
    let deep = ["a-rather-long-directory-name"; 10].join("/");
    let s = Setup::at("exec-long", &deep);
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    assert!(canon.as_os_str().len() > 255, "{}", canon.display());
    // Ports of its own, so the tests beside it that count theirs see none.
    s.repo.write(
        ".config/sluice.toml",
        &POLICY.replace("\"41500\"", "\"43100\""),
    );
    s.repo.git(&["commit", "-q", "-am", "ports of its own"]);
    s.repo.git(&["branch", "-f", "bolt/x", "main"]);

    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        s.results().contains("\"outcome\":\"pass\""),
        "{}",
        s.results()
    );
    let out = s.sluice(&["exec", "bolt/x", "--", "sh", "-c", "echo $SLUICE_PORT_WEB"]);
    assert!(out.status.success(), "{}", text(&out));
    // 35 as amended, rule core.30: its scratch is the user's directory,
    // here the tests', then the repository's 10 hex digits and its own,
    // and holds nothing of the worktree's path or the repository's.
    let out = s.sluice(&["exec", "bolt/x", "--", "sh", "-c", "echo $SLUICE_TMP"]);
    assert!(out.status.success(), "{}", text(&out));
    let printed = String::from_utf8_lossy(&out.stdout);
    let user = support::test_user_scratch();
    let tmp = printed
        .lines()
        .find(|l| Path::new(l).starts_with(&user))
        .unwrap_or_else(|| {
            panic!(
                "no scratch under {} printed: {}",
                user.display(),
                text(&out)
            )
        });
    let rest = Path::new(tmp)
        .strip_prefix(&user)
        .unwrap()
        .to_string_lossy();
    let hex = |h: &str| h.len() == 10 && h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
    let parts: Vec<&str> = rest.split('/').collect();
    assert!(parts.len() == 2 && parts.iter().all(|p| hex(p)), "{tmp}");
    let repo = s.repo.dir().file_name().unwrap().to_string_lossy();
    assert!(!tmp.contains(repo.as_ref()), "{tmp}");
    assert!(!tmp.contains("a-rather-long-directory-name"), "{tmp}");

    let out = s.sluice(&["status", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        doc["worktrees"][0]["worktree"],
        canon.to_str().unwrap(),
        "{doc}"
    );
    assert_eq!(doc["worktrees"][0]["removed"], false, "{doc}");

    let mut names = file_names(&s.repo.dir().join(".git/sluice"));
    names.extend(file_names(s.state.path()));
    assert!(names.iter().any(|n| n.contains("%~")), "{names:?}");
    for n in &names {
        assert!(n.len() <= 255, "{n}");
    }

    let out = s.sluice(&["release"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["worktrees"].as_array().map_or(0, Vec::len), 0, "{doc}");
}

/// 35 as amended, the C.4 sandbox row, rule core.30: under the sandbox, a
/// worktree at a long path binds a tool's session socket in a folder under
/// its scratch, which is under the host user's own directory, the binary
/// given none of the tests', so the socket's path fits macOS's 104 bytes.
#[cfg(target_os = "macos")]
#[test]
fn exec_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch() {
    let deep = ["a-rather-long-directory-name"; 10].join("/");
    let s = Setup::at("exec-sandbox-socket", &deep);
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    assert!(canon.as_os_str().len() > 255, "{}", canon.display());
    s.policy(&format!(
        "{}\n[executor]\nkind = \"sandbox\"\nnetwork = \"localhost\"\n",
        POLICY.replace("\"41500\"", "\"43300\"")
    ));

    let bind = "d=\"$SLUICE_TMP/x/sessions/work\" && mkdir -p \"$d\" && \
        /usr/bin/perl -MIO::Socket::UNIX -e \
        'IO::Socket::UNIX->new(Local => $ARGV[0], Listen => 1) or die \"binding $ARGV[0]: $!\\n\"' \
        \"$d/x-client.sock\" && echo \"$d/x-client.sock\"";
    let out = s
        .command(&["exec", "bolt/x", "--", "sh", "-c", bind])
        .env_remove(sluice::profiles::places::USER_SCRATCH_VAR)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let user = sluice::profiles::places::user_scratch().unwrap();
    let printed = String::from_utf8_lossy(&out.stdout);
    let socket = printed
        .lines()
        .find(|l| l.ends_with("/x-client.sock"))
        .unwrap_or_else(|| panic!("no socket's path printed: {}", text(&out)));
    assert!(Path::new(socket).starts_with(&user), "{socket}");
    assert!(socket.len() < 104, "{socket}: {} bytes", socket.len());
}

/// 35 as amended, the C.4 sandbox row, rule core.30: under the sandbox, a
/// landing's check binds a tool's session socket in a folder under its
/// candidate's scratch, which is under the host user's own directory, the
/// binary given none of the tests', so the socket's path fits macOS's 104
/// bytes for a submission from a worktree at a long path. The check fails
/// where its scratch is elsewhere or the socket's path is longer, so the
/// submission lands only where it fits.
#[cfg(target_os = "macos")]
#[test]
fn a_landing_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch() {
    let deep = ["a-rather-long-directory-name"; 10].join("/");
    let s = Setup::at("land-sandbox-socket", &deep);
    let user = sluice::profiles::places::user_scratch().unwrap();
    s.repo.write(
        "check.sh",
        &format!(
            "case \"$SLUICE_TMP\" in '{user}'/*) ;; \
             *) echo \"$SLUICE_TMP is not under {user}\" >&2; exit 1 ;; esac\n\
             d=\"$SLUICE_TMP/x/sessions/work\"\n\
             mkdir -p \"$d\"\n\
             s=\"$d/x-client.sock\"\n\
             [ ${{#s}} -lt 104 ] || {{ echo \"$s: ${{#s}} bytes\" >&2; exit 1; }}\n\
             /usr/bin/perl -MIO::Socket::UNIX -e \
             'IO::Socket::UNIX->new(Local => $ARGV[0], Listen => 1) or die \"binding $ARGV[0]: $!\\n\"' \
             \"$s\"\n",
            user = user.display()
        ),
    );
    s.policy(&format!(
        "{}\n[executor]\nkind = \"sandbox\"\nnetwork = \"localhost\"\n",
        portless()
    ));

    // No session run first, so no pass of one stands for the candidate's.
    let out = s
        .command(&["submit", "bolt/x", "--json"])
        .env_remove(sluice::profiles::places::USER_SCRATCH_VAR)
        .env("SLUICE_ROUND_SLEEP", "record")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let head = s.repo.git(&["rev-parse", "unit/a"]);
    let landed = s
        .repo
        .git_raw(&["merge-base", "--is-ancestor", head.trim(), "bolt/x"]);
    assert!(landed.status.success(), "not landed: {}", text(&out));
    let results = s.results();
    let ran: Vec<serde_json::Value> = results
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        ran.iter().any(|l| l["outcome"] == "pass"
            && l["executor"]["kind"] == "sandbox"
            && l.get("worktree").is_none()),
        "no candidate's pass under the sandbox: {results}"
    );
}

/// 92: a clean worktree's run of the checks is recorded under its keys
/// with its executor and host, as a run on its head from its worktree; a
/// dirty one runs and records nothing.
#[test]
fn a_clean_run_of_the_checks_is_recorded_and_a_dirty_one_is_not() {
    let s = Setup::new("exec-checks");
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let results = s.results();
    let lines: Vec<serde_json::Value> = results
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1, "{results}");
    let head = s.repo.git(&["rev-parse", "unit/a"]);
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    let l = &lines[0];
    assert_eq!(l["kind"], "ran");
    assert_eq!(l["task"], "sh check.sh");
    assert_eq!(l["outcome"], "pass");
    assert_eq!(l["branch"], "bolt/x");
    assert_eq!(l["combination"], head.trim());
    assert_eq!(l["worktree"], canon.to_str().unwrap());
    assert_eq!(l["executor"]["kind"], "open");
    assert_eq!(l["executor"]["confined"], false);
    assert!(l["host"].is_string());

    std::fs::write(s.worktree.join("a.txt"), "changed").unwrap();
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("not recorded: worktree is not clean"),
        "{}",
        text(&out)
    );
    assert_eq!(s.results(), results, "a dirty run records nothing");
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(s.repo.dir().join(format!(
            ".git/sluice/.places/{}.json",
            sluice::runs::flat_name(&format!("session/{}", canon.display()))
        )))
        .unwrap(),
    )
    .unwrap();
    let tmp = PathBuf::from(record["scratch"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(tmp.join("checked")).unwrap(),
        "checked\nchecked\n",
        "both runs ran, with one scratch directory"
    );
}

/// 138, design D1 (the check-grant spec's "The open executor"): each
/// result `sluice exec` records of a clean worktree's run under the open
/// executor carries the open executor's grant.
#[test]
fn exec_records_the_open_executors_grant() {
    let s = Setup::new("exec-open-grant");
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let results = s.results();
    assert!(!results.is_empty(), "{}", text(&out));
    for l in results.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        assert_eq!(v["executor"]["kind"], "open", "{l}");
        assert_eq!(v["executor"]["grant"], "open:1", "{l}");
    }
}

/// The grant each run of the session's place at `worktree` recorded in
/// its request, oldest first.
fn run_grants(s: &Setup) -> Vec<String> {
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    let dir = s
        .repo
        .dir()
        .join(".git/sluice/.runs")
        .join(sluice::runs::flat_name(&format!(
            "session/{}",
            canon.display()
        )));
    let mut runs: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let n = e.file_name().to_str()?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    runs.sort();
    runs.into_iter()
        .map(|(_, d)| {
            let r: serde_json::Value =
                serde_json::from_slice(&std::fs::read(d.join("request.json")).unwrap()).unwrap();
            r["meta"]["executor"]["grant"].as_str().unwrap().to_string()
        })
        .collect()
}

/// 138, 139, S66; record 0072; design D1 of the change
/// `the-gate-sandbox-lets-the-tools-write` (the run-outcome spec's "S66"):
/// a sandboxed branch's test that writes a folder outside the worktree
/// fails in the first `sluice exec`, which records the fail and names the
/// write the sandbox refused beside it; once the host's `[[caches]]`
/// declares the folder, the second records its pass under a wider grant,
/// which no reading weighs with the fail.
#[cfg(target_os = "macos")]
#[test]
fn exec_records_a_fail_the_sandbox_refused_with_its_words_and_its_pass_under_a_wider_grant() {
    let s = Setup::new("exec-sandbox-refused");
    s.policy(&format!(
        "{}\n[executor]\nkind = \"sandbox\"\nnetwork = \"localhost\"\n",
        POLICY.replace("\"41500\"", "\"43400\"")
    ));
    let outside = s.config.path().join("shared");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        s.worktree.join("check.sh"),
        format!(
            "/bin/mkdir -p '{0}/made' && /usr/bin/touch '{0}/made/x'\n",
            outside.display()
        ),
    )
    .unwrap();
    let git = |args: &[&str]| {
        let mut a = vec!["-C", s.worktree.to_str().unwrap()];
        a.extend(args);
        s.repo.git(&a)
    };
    git(&["commit", "-q", "-am", "the check writes outside"]);

    let out = s.sluice(&["exec", "bolt/x"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(!said.contains("could not run"), "{said}");
    assert!(
        said.contains("sluice's sandbox refused the run file-write-create "),
        "{said}"
    );
    assert!(said.contains("/made (by mkdir)"), "{said}");
    let failed = s.results();
    assert!(
        failed.contains("\"outcome\":\"fail\""),
        "the fail recorded: {failed}"
    );

    std::fs::create_dir_all(s.config.path().join("sluice")).unwrap();
    std::fs::write(
        s.config.path().join("sluice/config.toml"),
        format!("[[caches]]\ndir = {:?}\n", outside.display().to_string()),
    )
    .unwrap();
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let results = s.results();
    let lines: Vec<serde_json::Value> = results
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{results}");
    assert_eq!(lines[1]["task"], "sh check.sh");
    assert_eq!(lines[1]["outcome"], "pass");
    let grants = run_grants(&s);
    assert_eq!(grants.len(), 2, "{grants:?}");
    assert_ne!(grants[0], grants[1], "a wider grant");
    assert_eq!(lines[0]["executor"]["grant"], grants[0].as_str());
    assert_eq!(lines[1]["executor"]["grant"], grants[1].as_str());
}

/// Session-runs-1 clause 97, 123: a worktree removed with `rm -rf`, with
/// nothing released, is no session's. The next `sluice exec` on the host,
/// from another worktree, runs the release its runs stored, once, in its
/// scratch directory with its port, the policy's toolchain not run again
/// in its name, and frees its place: its block of ports goes to the next
/// worktree, and the branch's log names it `gone` with its release run. A
/// third `sluice exec` runs its release no more. `sluice status` lists
/// every worktree held, and the removed one only until it is freed.
#[test]
fn the_next_exec_runs_a_removed_worktrees_stored_release_once() {
    let s = Setup::new("exec-removed");
    let marks = Scratch::new("exec-removed-marks");
    // Each says where it ran, so a run in the removed worktree's name
    // shows, and the release the port it was given.
    let write = |name: &str| {
        let script = marks.path().join(format!("{name}.sh"));
        let log = marks.path().join(name);
        std::fs::write(
            &script,
            format!("echo \"$(pwd -P) $SLUICE_PORT_WEB\" >> {}\n", log.display()),
        )
        .unwrap();
        (script, log)
    };
    let (release, released) = write("released");
    let (toolchain, tooled) = write("toolchain");
    s.repo.write(
        ".config/sluice.toml",
        // Ports of its own: a block it frees is free only if no test
        // beside it probes the same ports at that moment.
        &POLICY
            .replace(
                "[tests]\n",
                &format!(
                    "[tests]\nrelease = \"sh {}\"\ntoolchain = [\"/bin/sh\", \"{}\"]\n",
                    release.display(),
                    toolchain.display()
                ),
            )
            .replace("\"41500\"", "\"43200\""),
    );
    s.repo.git(&["commit", "-q", "-am", "a release command"]);
    s.repo.git(&["branch", "-f", "bolt/x", "main"]);
    let other = s._sessions.path().join("b");
    s.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/b",
        other.to_str().unwrap(),
        "bolt/x",
    ]);
    let sluice_in = |dir: &Path, args: &[&str]| -> Output {
        support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
            .args(args)
            .current_dir(dir)
            .env("XDG_CONFIG_HOME", s.config.path())
            .env("XDG_STATE_HOME", s.state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let port = |dir: &Path| -> String {
        let out = sluice_in(
            dir,
            &["exec", "bolt/x", "--", "sh", "-c", "echo $SLUICE_PORT_WEB"],
        );
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.parse::<u16>().is_ok())
            .unwrap()
            .to_string()
    };
    let a_port = port(&s.worktree);
    let canon_a = std::fs::canonicalize(&s.worktree).unwrap();

    // Held, however long since its last run, while the worktree exists.
    let out = sluice_in(s.repo.dir(), &["status"]);
    assert!(out.status.success(), "{}", text(&out));
    let listed = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(listed.contains(canon_a.to_str().unwrap()), "{listed}");
    assert!(
        listed.contains(&format!("SLUICE_PORT_WEB={a_port}")),
        "{listed}"
    );
    assert!(listed.contains("last run 20"), "{listed}");

    // Its runs ran the toolchain in it, as every run does.
    let ran_in = |log: &Path| std::fs::read_to_string(log).unwrap_or_default();
    assert!(ran_in(&tooled).contains(canon_a.to_str().unwrap()));
    std::fs::remove_file(&tooled).unwrap();

    // Removed with nothing released: still held, and listed as removed.
    std::fs::remove_dir_all(&s.worktree).unwrap();
    let out = sluice_in(s.repo.dir(), &["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["worktrees"][0]["removed"], true, "{doc}");
    assert!(!released.exists());

    // The next run, in another worktree, runs its stored release first,
    // once, from the scratch directory its record names with its port, and
    // frees it (123, rule core.30).
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(s.repo.dir().join(format!(
            ".git/sluice/.places/{}.json",
            sluice::runs::flat_name(&format!("session/{}", canon_a.display()))
        )))
        .unwrap(),
    )
    .unwrap();
    let scratch = std::fs::canonicalize(record["scratch"].as_str().unwrap()).unwrap();
    let out = sluice_in(&other, &["exec", "bolt/x", "--", "true"]);
    assert!(out.status.success(), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains(&format!(
            "freed {}: it no longer exists; its release ran once from its scratch directory; its runs ended, its ports free",
            canon_a.display()
        )),
        "{said}"
    );
    let once = format!("{} {a_port}\n", scratch.display());
    assert_eq!(ran_in(&released), once);
    assert!(!scratch.exists(), "{} left", scratch.display());
    assert!(
        !ran_in(&tooled).contains(canon_a.to_str().unwrap())
            && !ran_in(&tooled).contains(scratch.to_str().unwrap()),
        "the toolchain ran in its name: {}",
        ran_in(&tooled)
    );

    // Its block of ports is free: the next worktree takes it.
    assert_eq!(port(&other), a_port);
    let out = sluice_in(s.repo.dir(), &["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let held = doc["worktrees"].as_array().unwrap();
    assert_eq!(held.len(), 1, "{doc}");
    assert_eq!(
        held[0]["worktree"],
        std::fs::canonicalize(&other).unwrap().to_str().unwrap()
    );
    assert_eq!(held[0]["removed"], false);
    // Logged gone where the branch's releases are, naming the worktree,
    // and never named again.
    let log = std::fs::read_to_string(s.repo.dir().join(".git/sluice/bolt/x/log.jsonl")).unwrap();
    let named: Vec<&str> = log
        .lines()
        .filter(|l| l.contains(&format!("\"worktree\":\"{}\"", canon_a.display())))
        .collect();
    assert_eq!(named.len(), 1, "{log}");
    assert!(named[0].contains("\"event\":\"gone\""), "{log}");
    assert!(named[0].contains("\"ran\":true"), "{log}");
    let out = sluice_in(&other, &["exec", "bolt/x", "--", "true"]);
    assert!(
        !text(&out).contains(canon_a.to_str().unwrap()),
        "{}",
        text(&out)
    );
    assert_eq!(ran_in(&released), once, "its release ran again");
}

/// Session-runs-1 clause 95, rule core.10, record 0120: `sluice release`
/// in a worktree where no `sluice exec` ever ran runs the release command
/// `main`'s policy names, in the worktree, and answers as a held
/// worktree's release does: released, exit 0, when it passed, and what it
/// printed, exit 1, when it failed. Either way sluice holds nothing for the
/// worktree after. A policy that cannot be read fails the release, naming
/// `main`, with nothing run.
#[test]
fn an_unheld_worktrees_release_runs_its_release_command() {
    let s = Setup::new("exec-unheld-release");
    let marks = Scratch::new("exec-unheld-release-marks");
    let released = marks.path().join("released");
    let release = marks.path().join("release.sh");
    std::fs::write(&release, format!("pwd -P >> {}\n", released.display())).unwrap();
    let fail = marks.path().join("fail.sh");
    std::fs::write(&fail, "echo emulator still answers\nexit 3\n").unwrap();
    let releasing = |script: &Path| {
        POLICY.replace(
            "[tests]\n",
            &format!("[tests]\nrelease = \"sh {}\"\n", script.display()),
        )
    };
    let canon = std::fs::canonicalize(&s.worktree).unwrap();
    let held = || {
        let out = s
            .command(&["status"])
            .current_dir(s.repo.dir())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // A release that leaves a mark: run in the worktree, and released.
    s.policy(&releasing(&release));
    let out = s.sluice(&["release"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        said.starts_with(&format!("released {}", canon.display())),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read_to_string(&released).unwrap(),
        format!("{}\n", canon.display())
    );
    let status = held();
    assert!(
        status.contains("no worktree holds ports or runs here"),
        "{status}"
    );

    // A failing release: exit 1 with what it printed, and nothing held.
    s.policy(&releasing(&fail));
    let out = s.sluice(&["release"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("emulator still answers"),
        "{}",
        text(&out)
    );
    let status = held();
    assert!(
        status.contains("no worktree holds ports or runs here"),
        "{status}"
    );

    // A policy that cannot be read: exit 1 naming main, and nothing run.
    s.policy(&releasing(&release).replace("[tests]\n", "[tests]\nnot_a_key = true\n"));
    let out = s.sluice(&["release"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("main"), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(&released).unwrap(),
        format!("{}\n", canon.display()),
        "a release ran under a policy that cannot be read"
    );
}

/// Every path kept under `dir`, walked whole, relative to it.
fn kept(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries {
            let path = e.unwrap().path();
            out.push(path.strip_prefix(dir).unwrap().display().to_string());
            if path.is_dir() {
                todo.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Session-runs-1 clause 95, rule core.10, record 0120 as amended
/// 2026-11-17: `sluice release` in a worktree sluice holds nothing for,
/// under a policy that names no release command, reads only: it says
/// `nothing to release`, exits 0, and makes nothing, neither the user's
/// scratch directory nor a record, a lock or a folder under the
/// repository's `sluice/`, so a caller whose sandbox lets it write only
/// its place, that place's scratch and its caches is not refused (deneb,
/// 2026-11-17: flume's s107 teardown answered `creating
/// /private/tmp/sluice-501/<key>: Operation not permitted`).
#[test]
fn an_unheld_release_with_nothing_to_free_makes_nothing() {
    let s = Setup::new("exec-unheld-nothing");
    let user = Scratch::new("exec-unheld-nothing-user");
    let scratch = user.path().join("sluice-user");
    let sluice = s.repo.dir().join(".git/sluice");
    let before = kept(&sluice);
    let state_before = kept(s.state.path());

    let out = s.sluice_with(&[("SLUICE_USER_SCRATCH", &scratch)], &["release"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "nothing to release\n");
    assert!(!scratch.exists(), "{:?}", kept(&scratch));
    assert_eq!(kept(&sluice), before);
    // No host process was told, so none was started (135; D10).
    assert_eq!(kept(s.state.path()), state_before);
}

/// Session-runs-1 clauses 92, 97, rule core.18: each run leaves on the
/// worktree's entry in `sluice status --json` and `sluice overview --json`
/// the head and outcome of the last run there, recorded only for a clean
/// run of the checks; the next run replaces it and the release removes it.
#[test]
fn a_worktrees_last_result_says_its_head_outcome_and_whether_it_was_recorded() {
    let s = Setup::new("exec-last-result");
    let head = |s: &Setup| s.repo.git(&["rev-parse", "unit/a"]).trim().to_string();
    let git = |args: &[&str]| {
        let mut a = vec!["-C", s.worktree.to_str().unwrap()];
        a.extend(args);
        s.repo.git(&a)
    };
    let held = |s: &Setup| -> serde_json::Value {
        let out = s.sluice(&["status", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["worktrees"][0].clone()
    };
    let last = |s: &Setup| held(s)["last_result"].clone();
    let expect = |s: &Setup, head: &str, kind: &str, outcome: &str, recorded: bool| {
        let l = last(s);
        assert_eq!(l["head"], head, "{l}");
        assert_eq!(l["kind"], kind, "{l}");
        assert_eq!(l["outcome"], outcome, "{l}");
        assert_eq!(l["recorded"], recorded, "{l}");
        assert!(l["at"].as_str().unwrap().starts_with("20"), "{l}");
    };

    // A clean pass, recorded; the overview lists the same.
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let passed = head(&s);
    expect(&s, &passed, "checks", "pass", true);
    let entry = held(&s);
    for field in ["last_run", "ports", "removed", "worktree", "branch"] {
        assert!(!entry[field].is_null(), "{field}: {entry}");
    }
    let out = s.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["sessions"][0], entry, "{doc}");
    let out = s.sluice(&["status"]);
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        said.contains(&format!("last result passed on {}, recorded", &passed[..7])),
        "{said}"
    );

    // A clean fail, recorded, replaces it.
    std::fs::write(s.worktree.join("check.sh"), "exit 1\n").unwrap();
    git(&["commit", "-q", "-am", "a failing check"]);
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(!out.status.success(), "{}", text(&out));
    let failed = head(&s);
    expect(&s, &failed, "checks", "fail", true);

    // An unclean pass is not recorded.
    std::fs::write(s.worktree.join("check.sh"), "true\n").unwrap();
    git(&["commit", "-q", "-am", "a passing check"]);
    std::fs::write(s.worktree.join("a.txt"), "changed").unwrap();
    let before = s.results();
    let out = s.sluice(&["exec", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.results(), before);
    expect(&s, &head(&s), "checks", "pass", false);

    // Nor is a command that exits 0.
    let out = s.sluice(&["exec", "bolt/x", "--", "true"]);
    assert!(out.status.success(), "{}", text(&out));
    expect(&s, &head(&s), "command", "pass", false);
    let out = s.sluice(&["status"]);
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(said.contains("last result command passed on"), "{said}");

    // A run its time limit ends.
    s.repo.write(
        ".config/sluice.toml",
        &POLICY.replace("timeout = \"1m\"", "timeout = \"1s\""),
    );
    s.repo.git(&["commit", "-q", "-am", "a short time limit"]);
    s.repo.git(&["branch", "-f", "bolt/x", "main"]);
    let out = s.sluice(&["exec", "bolt/x", "--", "sleep", "30"]);
    assert!(!out.status.success(), "{}", text(&out));
    expect(&s, &head(&s), "command", "timed_out", false);

    // The release removes it.
    let out = s.sluice(&["release"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(held(&s).is_null(), "released");
}

/// The policy with no ports, so a test of the host's bound probes none.
fn portless() -> String {
    POLICY.replace("ports = [\"web\"]\nports_from = \"41500\"\n", "")
}

/// 71, 89; the exec-checks spec's "An agent waits on another repository's
/// queue" and the run-bound spec's "Another repository's agent run holds
/// the bound": on a host whose bound is one, another repository's `sluice
/// exec` holds the one run; this repository's waits, saying so, and runs
/// once that one ends.
#[test]
fn an_exec_waits_for_another_repositorys_run_on_the_host() {
    let a = Setup::at("exec-host-a", "a");
    let b = Setup::at("exec-host-b", "b");
    a.policy(&portless());
    b.policy(&portless());
    host_bound(&a.config, 1);

    let mut holding = b.spawn_on(
        &a,
        &[
            "exec",
            "bolt/x",
            "--",
            "sh",
            "-c",
            "touch started; while [ ! -e go ]; do sleep 0.05; done",
        ],
    );
    eventually("the other repository's run starting", || {
        b.worktree.join("started").exists()
    });

    let mut waiting = a.spawn_on(&a, &["exec", "bolt/x", "--", "sh", "-c", "echo ran > ran"]);
    let said = lines_of(&mut waiting);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let line = said
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .expect("exec says it waits");
        if line == "waiting: the host is at its bound" {
            break;
        }
    }
    assert!(!a.worktree.join("ran").exists(), "it waits before it runs");

    std::fs::write(b.worktree.join("go"), "").unwrap();
    assert!(holding.wait().unwrap().success());
    assert!(waiting.wait().unwrap().success());
    assert_eq!(
        std::fs::read_to_string(a.worktree.join("ran")).unwrap(),
        "ran\n"
    );
}

/// 71; the exec-checks spec's "Another repository's submission": another
/// repository's combination running on the host counts in this
/// repository's `room.queue`, and no branch here shows it under test.
#[test]
fn the_overview_counts_another_repositorys_combination_in_its_room() {
    let a = Setup::at("exec-room-a", "a");
    let b = Setup::at("exec-room-b", "b");
    let marks = Scratch::new("exec-room-marks");
    let (started, go) = (marks.path().join("started"), marks.path().join("go"));
    a.policy(&portless());
    b.policy(&portless());
    b.repo.write(
        "check.sh",
        &format!(
            "touch '{}'; while [ ! -e '{}' ]; do sleep 0.05; done\n",
            started.display(),
            go.display()
        ),
    );
    b.repo.git(&["commit", "-q", "-am", "a check that holds"]);
    b.repo.git(&["branch", "-f", "bolt/x", "main"]);
    host_bound(&a.config, 3);

    let on_host = |s: &Setup, args: &[&str]| {
        let out = s.spawn_on(&a, args).wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    on_host(&b, &["submit", "bolt/x", "--no-wait", "--json"]);
    on_host(&b, &["step", "bolt/x"]);
    eventually("the other repository's combination running", || {
        started.exists()
    });

    let doc: serde_json::Value =
        serde_json::from_str(&on_host(&a, &["overview", "--json"])).unwrap();
    std::fs::write(&go, "").unwrap();
    assert_eq!(doc["room"]["bound"], 3, "{doc}");
    assert_eq!(doc["room"]["queue"], 1, "{doc}");
    assert_eq!(doc["room"]["sessions"], 0, "{doc}");
    for branch in doc["branches"].as_array().unwrap() {
        assert_eq!(branch["candidates"], serde_json::json!([]), "{doc}");
    }
    // The run goes on after the test's reads: its host process is
    // stopped, which ends it.
    support::guard::stop_host_processes(a.state.path());
}

/// 84, record 0074 (design D6); the host-requirements spec's "The tests
/// are refused" and "A command runs": on a host whose container engine
/// does not answer, the tests of a branch that asks for one are refused,
/// naming where the host looked, and nothing runs or is recorded; a
/// command after `--` runs.
#[test]
fn exec_refuses_the_tests_of_a_branch_whose_engine_does_not_answer() {
    let s = Setup::new("exec-no-engine");
    s.policy(&format!("{POLICY}\n[executor]\ndocker = true\n"));
    let none = Scratch::short();
    let socket = none.path().join("e.sock");
    let env = [(ENGINE_SOCKETS, socket.as_path())];
    let out = s.sluice_with(&env, &["exec", "bolt/x"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains(&format!(
            "refused: no container engine answers: no socket at {}; \
             bolt/x's tests ask for one ([executor] docker = true)",
            socket.display()
        )),
        "{said}"
    );
    assert!(!said.contains("running the checks"), "{said}");
    assert_eq!(s.results(), "", "nothing is recorded");
    assert!(s.sessions().is_empty(), "nothing runs");

    let out = s.sluice_with(&env, &["exec", "bolt/x", "--", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

/// 114, 139; record 0074 (design D5); the host-requirements spec's "The
/// engine stopped during the run": a check that takes the engine away and
/// fails is not run, with why, and nothing is recorded under its key;
/// the check that passed beside it keeps its pass.
#[test]
fn a_check_that_fails_after_the_engine_went_is_not_run_and_a_pass_is_recorded() {
    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 200 OK");
    let s = moon_setup(
        "exec-engine-gone",
        &format!("rm -f '{}'\nexit 1\n", socket.display()),
    );
    let out = s.sluice_with(&[(ENGINE_SOCKETS, socket.as_path())], &["exec", "bolt/x"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(&format!(
            "could not run: a:test: after the run, no container engine answers: \
             no socket at {}",
            socket.display()
        )),
        "{said}"
    );
    assert_eq!(
        recorded(&s),
        [("b:test".to_string(), "pass".to_string())],
        "{said}"
    );
}

/// Record 0074; the host-requirements spec's "A test that says docker is
/// down": what a check prints is never read as the engine gone; while
/// the engine answers after the run, its fail is recorded under its key.
#[test]
fn a_check_that_says_the_engine_is_down_while_it_answers_is_recorded_as_failed() {
    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 200 OK");
    let s = moon_setup(
        "exec-engine-says",
        "echo 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock. \
         Is the docker daemon running?'\nexit 1\n",
    );
    let out = s.sluice_with(&[(ENGINE_SOCKETS, socket.as_path())], &["exec", "bolt/x"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("failed: a:test"), "{said}");
    let mut got = recorded(&s);
    got.sort();
    assert_eq!(
        got,
        [
            ("a:test".to_string(), "fail".to_string()),
            ("b:test".to_string(), "pass".to_string())
        ],
        "{said}"
    );
}

/// Where the binary reads the active system's link in place of the
/// host's `/run/current-system` (record 0097, design D8).
const SYSTEM_LINK: &str = "SLUICE_SYSTEM_LINK";

/// A link in `dir` standing for the host's `/run/current-system`, to
/// `<dir>/aaa`: the link, and the store path it names.
fn system_link(dir: &Path) -> (PathBuf, PathBuf) {
    let (link, system) = (dir.join("current-system"), dir.join("aaa"));
    std::os::unix::fs::symlink(&system, &link).unwrap();
    (link, system)
}

/// The worktrees `sluice status` lists a checkout of.
fn worktrees(s: &Setup) -> usize {
    let out = s.sluice(&["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    doc["worktrees"].as_array().map_or(0, Vec::len)
}

/// 84, 114, 139; record 0097 (design D5, D7); the host-requirements
/// spec's "The tests": a check that switches the host's system and fails
/// is not run, naming the system before and after, and nothing is
/// recorded under its key; the check that passed beside it keeps its
/// pass, and nothing is released.
#[test]
fn a_check_that_fails_after_the_hosts_system_was_switched_is_not_run_and_a_pass_is_recorded() {
    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 200 OK");
    let (link, from) = system_link(dir.path());
    let to = dir.path().join("bbb");
    let s = moon_setup(
        "exec-system-switched",
        &format!("ln -sfn '{}' '{}'\nexit 1\n", to.display(), link.display()),
    );
    let env = [
        (ENGINE_SOCKETS, socket.as_path()),
        (SYSTEM_LINK, link.as_path()),
    ];
    let out = s.sluice_with(&env, &["exec", "bolt/x"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(&format!(
            "could not run: a:test: the host's system was switched during the run, \
             from {} to {}",
            from.display(),
            to.display()
        )),
        "{said}"
    );
    assert_eq!(
        recorded(&s),
        [("b:test".to_string(), "pass".to_string())],
        "{said}"
    );
    assert!(s.worktree.exists());
    assert_eq!(worktrees(&s), 1, "the worktree's checkout stays");
}

/// 84; rules core.37, core.38; record 0097; the host-requirements spec's
/// "A test that says the host was switched": what a check prints is
/// never read as a switch; while the host's system stood, its fail is
/// recorded under its key.
#[test]
fn a_check_that_says_the_system_was_switched_while_it_stood_is_recorded_as_failed() {
    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 200 OK");
    let (link, _) = system_link(dir.path());
    let s = moon_setup(
        "exec-system-says",
        "echo \"the host's system was switched during the run, \
         from /nix/store/aaa-darwin-system to /nix/store/bbb-darwin-system\"\nexit 1\n",
    );
    let env = [
        (ENGINE_SOCKETS, socket.as_path()),
        (SYSTEM_LINK, link.as_path()),
    ];
    let out = s.sluice_with(&env, &["exec", "bolt/x"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("failed: a:test"), "{said}");
    assert!(!said.contains("could not run"), "{said}");
    let mut got = recorded(&s);
    got.sort();
    assert_eq!(
        got,
        [
            ("a:test".to_string(), "fail".to_string()),
            ("b:test".to_string(), "pass".to_string())
        ],
        "{said}"
    );
}

/// Design D5; the host-requirements spec's "An engine at a TCP address"
/// and "Nothing answers at the TCP address": with `docker = true` and
/// `DOCKER_HOST` naming a fake engine's TCP port, `sluice exec` runs the
/// tests, which see the address; with nothing listening there, it refuses
/// them, naming the address. The look is held to that address alone, so
/// no engine of the host's own answers in its place.
#[test]
fn exec_looks_for_the_engine_at_a_tcp_address() {
    let s = Setup::new("exec-engine-tcp");
    s.policy(&format!("{POLICY}\n[executor]\ndocker = true\n"));
    // The session runs its worktree's own check.
    std::fs::write(
        s.worktree.join("check.sh"),
        "echo \"$DOCKER_HOST\" > \"$SLUICE_CHECKOUT/reached\"\n",
    )
    .unwrap();
    let tree = s.worktree.to_str().unwrap();
    s.repo
        .git(&["-C", tree, "commit", "-q", "-am", "a check that says where"]);
    let at = |port: u16| format!("tcp://127.0.0.1:{port}");
    // Where the engine is looked for is the host process's own setting:
    // each exec starts its own host process, with the address it names.
    let exec = |address: &str| {
        support::guard::stop_host_processes(s.state.path());
        let env = [
            ("DOCKER_HOST", Path::new(address)),
            (ENGINE_SOCKETS, Path::new(address)),
        ];
        s.sluice_with(&env, &["exec", "bolt/x"])
    };

    let engine = at(support::tcp_engine("HTTP/1.0 200 OK"));
    let out = exec(&engine);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(s.worktree.join("reached")).unwrap(),
        format!("{engine}\n")
    );

    let refusal = |closed: &str| {
        format!(
            "refused: no container engine answers: {closed} refused the connection; \
             bolt/x's tests ask for one ([executor] docker = true)"
        )
    };
    let (closed, out) = support::at_a_closed_port(
        |port| {
            let _ = std::fs::remove_file(s.worktree.join("reached"));
            exec(&at(port))
        },
        |port, out| text(out).contains(&refusal(&at(port))),
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains(&refusal(&at(closed))), "{said}");
    assert!(!s.worktree.join("reached").exists(), "nothing runs");
}

/// resident-workers, "sluice host stopped while a session's checks run"
/// (the change `sluice-host-runs-a-sessions-checks`, design D4): a `sluice
/// exec` whose host process answers `hello` and closes the connection at
/// `exec` says that sluice's host process ended, reads and runs nothing
/// itself, and exits 3.
#[test]
fn a_command_whose_host_process_ends_before_it_answers_exits_3() {
    use std::io::Write;
    let s = Setup::new("exec-host-ends");
    let client = sluice::host_process::client::Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(&s.command(&[])),
    )
    .unwrap();
    let socket = client.paths().socket.clone();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&socket);
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let fake = {
        let (done, asked) = (done.clone(), asked.clone());
        std::thread::spawn(move || {
            let named = serde_json::json!({
                "version": sluice::VERSION,
                "pid": std::process::id(),
                "since": "2026-11-22T00:00:00.000Z",
            });
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let _ = writeln!(writer, "{named}");
                line.clear();
                let _ = reader.read_line(&mut line);
                let request: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let ask = request["ask"].as_str().unwrap_or_default().to_string();
                asked.lock().unwrap().push(ask.clone());
                match ask.as_str() {
                    // It ends before the command does.
                    "exec" => {
                        let _ = writeln!(writer, "{}", serde_json::json!({"stdout": "running\n"}));
                        drop(writer);
                    }
                    _ => {
                        let _ = writeln!(writer, "\"ok\"");
                    }
                }
            }
        })
    };
    let results = s.results();
    let out = s.sluice(&["exec", "bolt/x"]);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = fake.join();
    assert_eq!(*asked.lock().unwrap(), ["exec"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "running\n");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("sluice's host process") && err.contains("ended before the command did"),
        "{err}"
    );
    assert_eq!(s.results(), results);
    let _ = std::fs::remove_file(&socket);
}

/// What the host process's own branch protection upkeep runs of git in a
/// client: `protect.rs`'s reads of where the hook goes (124).
const UPKEEP: [&str; 3] = [
    "rev-parse --path-format=absolute --git-path hooks",
    "rev-parse --path-format=absolute --git-common-dir",
    "worktree list --porcelain",
];

/// A connection that holds `s`'s host process, started with the test's
/// own `PATH`, its toolchain and task runner on it, until it is dropped:
/// a subscription of no folder, which reads nothing.
fn host_held(s: &Setup) -> sluice::host_process::client::Connection {
    sluice::host_process::client::Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(&s.command(&[])),
    )
    .unwrap()
    .subscribe(Vec::new())
    .expect("the host process starts")
}

/// `c` with no toolchain and no task runner on its `PATH`: the system's
/// own folders, behind a `git` that records each call (the change
/// `sluice-host-runs-a-sessions-checks`, tasks).
fn without_tools<'a>(git: &support::CountingGit, c: &'a mut Command) -> &'a mut Command {
    c.env("PATH", "/usr/bin:/bin");
    git.on(c)
}

/// Each git the client `pid` ran is the branch protection's upkeep (the
/// change `sluice-host-runs-a-sessions-checks`, tasks).
fn ran_only_the_upkeep(git: &support::CountingGit, pid: u32) {
    for (_, call) in git.parented().into_iter().filter(|(p, _)| *p == pid) {
        assert!(
            UPKEEP.iter().any(|u| call.ends_with(u)),
            "the client ran git {call}"
        );
    }
}

/// `pid` and its parents, nearest first.
fn ancestors(pid: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut at = pid as i32;
    while at > 1 {
        chain.push(at as u32);
        at = sluice::workers::parent(at).unwrap_or(0);
    }
    chain
}

/// The pid of `s`'s host process, as its state names it.
fn host_pid(s: &Setup) -> u32 {
    let named = std::fs::read(s.state.path().join("sluice/host/version")).unwrap();
    let named: serde_json::Value = serde_json::from_slice(&named).unwrap();
    named["pid"].as_u64().unwrap() as u32
}

/// Whether `pid` runs, a zombie aside.
fn runs(pid: u32) -> bool {
    sluice::workers::started(pid as i32).is_some()
}

/// The pid a check wrote at `path`, once it did.
fn written_pid(path: &Path) -> u32 {
    let mut pid = 0;
    eventually("the check wrote its pid", || {
        pid = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0);
        pid != 0
    });
    pid
}

/// The runs that the host's room counts for sessions and previews.
fn sessions_running(s: &Setup) -> u64 {
    let out = s.sluice(&["overview", "--json"]);
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    doc["room"]["sessions"].as_u64().unwrap_or(0)
}

/// `sluice exec bolt/x -- sh -c 'echo $$ > <pid>; exec sleep <secs>'` in
/// `s`'s worktree, started, its output piped: the client, and where its
/// `sleep` writes its pid.
fn sleeping(s: &Setup, marks: &Scratch, secs: u32) -> (Child, PathBuf) {
    let pid = marks.path().join("pid");
    let script = format!("echo $$ > '{}'; exec sleep {secs}", pid.display());
    let child = s
        .command(&["exec", "bolt/x", "--", "sh", "-c", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    (child, pid)
}

/// resident-workers, "A session's checks" (the change
/// `sluice-host-runs-a-sessions-checks`, design D1;
/// `workers-end-with-their-owner@292e869158d6`): the parent chain of a
/// session's check reaches `sluice host` and not `sluice exec`, its group
/// is not the host process's, and the host's state names it.
#[test]
fn a_sessions_checks_are_a_worker_of_sluice_host() {
    let s = Setup::new("exec-worker");
    let marks = Scratch::new("exec-worker-marks");
    let (client, pid) = sleeping(&s, &marks, 30);
    let sleep = written_pid(&pid);
    let chain = ancestors(sleep);
    let host = host_pid(&s);
    assert!(
        chain.contains(&host),
        "sluice host {host} is not above the check: {chain:?}"
    );
    // The host process is a child of the command that started it: the
    // check's own chain is what lies below it.
    let below = &chain[..chain.iter().position(|p| *p == host).unwrap()];
    assert!(
        !below.contains(&client.id()),
        "the check runs under sluice exec: {chain:?}"
    );
    let group = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(sleep as i32)))
        .unwrap()
        .as_raw();
    let host_group = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(host as i32)))
        .unwrap()
        .as_raw();
    assert_ne!(group, host_group, "the check is in the host's own group");
    let record = s
        .state
        .path()
        .join("sluice/host/workers")
        .join(group.to_string());
    assert!(record.exists(), "no record of its group at {record:?}");
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(client.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    );
    let _ = client.wait_with_output();
    support::guard::stop_host_processes(s.state.path());
}

/// resident-workers, "The output reaches the client as it comes" (the
/// change `sluice-host-runs-a-sessions-checks`, design D1): with no
/// toolchain and no task runner on its `PATH`, the client prints `one`
/// before the check prints `two`, then `two`, and exits 4 as a command that
/// exits 4 did; it runs no git but the branch protection's upkeep.
#[test]
fn the_output_reaches_the_client_as_it_comes() {
    let s = Setup::new("exec-streams");
    let marks = Scratch::new("exec-streams-marks");
    let go = marks.path().join("go");
    let _held = host_held(&s);
    let git = support::CountingGit::new();
    let script = format!(
        "echo one; while [ ! -e '{}' ]; do sleep 0.05; done; echo two; exit 4",
        go.display()
    );
    let mut client = without_tools(
        &git,
        &mut s.command(&["exec", "bolt/x", "--", "sh", "-c", &script]),
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let pid = client.id();
    let said = lines_of(&mut client);
    let next = |what: &str| loop {
        let line = said
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| panic!("no {what}"));
        if line == what {
            break;
        }
    };
    next("one");
    assert!(!go.exists());
    std::fs::write(&go, "").unwrap();
    next("two");
    let out = client.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(4), "{}", text(&out));
    ran_only_the_upkeep(&git, pid);
}

/// resident-workers, "A session's checks wait for a slot" (the change
/// `sluice-host-runs-a-sessions-checks`, design D3): with `[runs] max` at
/// one and a combination's checks running, a session's checks say
/// `waiting: the host is at its bound`, and start only once the
/// combination's checks end.
#[test]
fn a_sessions_checks_take_a_slot_of_the_bound() {
    let a = Setup::at("exec-slot-a", "a");
    let b = Setup::at("exec-slot-b", "b");
    let marks = Scratch::new("exec-slot-marks");
    let (started, go, ran) = (
        marks.path().join("started"),
        marks.path().join("go"),
        marks.path().join("ran"),
    );
    a.policy(&portless());
    b.policy(&portless());
    b.repo.write(
        "check.sh",
        &format!(
            "touch '{}'; while [ ! -e '{}' ]; do sleep 0.05; done\n",
            started.display(),
            go.display()
        ),
    );
    b.repo.git(&["commit", "-q", "-am", "a check that holds"]);
    b.repo.git(&["branch", "-f", "bolt/x", "main"]);
    host_bound(&a.config, 1);
    let on_host = |s: &Setup, args: &[&str]| {
        let out = s.spawn_on(&a, args).wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    };
    on_host(&b, &["submit", "bolt/x", "--no-wait", "--json"]);
    on_host(&b, &["step", "bolt/x"]);
    eventually("the combination's checks running", || started.exists());

    let script = format!("touch '{}'", ran.display());
    let mut waiting = a.spawn_on(&a, &["exec", "bolt/x", "--", "sh", "-c", &script]);
    let said = lines_of(&mut waiting);
    loop {
        let line = said
            .recv_timeout(Duration::from_secs(60))
            .expect("exec says it waits");
        if line == "waiting: the host is at its bound" {
            break;
        }
    }
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        !ran.exists(),
        "the session's check ran beside the combination"
    );
    std::fs::write(&go, "").unwrap();
    assert!(waiting.wait().unwrap().success());
    assert!(ran.exists());
    support::guard::stop_host_processes(a.state.path());
}

/// resident-workers, "The caller's PATH" (the change
/// `sluice-host-runs-a-sessions-checks`, design D2): a session's run takes
/// its caller's `PATH`, not the one `sluice host` started with.
#[test]
fn a_session_run_takes_its_callers_path() {
    let s = Setup::new("exec-callers-path");
    let marks = Scratch::new("exec-callers-path-marks");
    let (a, b) = (marks.path().join("a"), marks.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let with_first = |dir: &Path| {
        let rest = std::env::var("PATH").unwrap_or_default();
        format!("{}:{rest}", dir.display())
    };
    let started = sluice::host_process::client::Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(s.command(&[]).env("PATH", with_first(&a))),
    )
    .unwrap()
    .subscribe(Vec::new())
    .expect("the host process starts");
    let out = s
        .command(&["exec", "bolt/x", "--", "sh", "-c", "echo \"$PATH\""])
        .env("PATH", with_first(&b))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let printed = String::from_utf8_lossy(&out.stdout);
    let path = printed
        .lines()
        .find(|l| l.contains(&*b.display().to_string()) || l.contains(&*a.display().to_string()))
        .unwrap_or_else(|| panic!("no PATH printed: {}", text(&out)));
    assert!(
        path.starts_with(&*b.display().to_string()),
        "the run's PATH is not its caller's: {path}"
    );
    drop(started);
    support::guard::stop_host_processes(s.state.path());
}

/// resident-workers, "Ctrl-C on a session's checks" (the change
/// `sluice-host-runs-a-sessions-checks`, design D4): within the grace of a
/// stopped call no `sleep` of the command runs, the run's slot is free,
/// and the worktree's last result says that the run stopped.
#[test]
fn ctrl_c_ends_a_sessions_checks() {
    let s = Setup::new("exec-ctrl-c");
    let marks = Scratch::new("exec-ctrl-c-marks");
    let (client, pid) = sleeping(&s, &marks, 60);
    let sleep = written_pid(&pid);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(client.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    );
    let out = client.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::STOPPED), "{}", text(&out));
    assert!(
        text(&out).contains("stopped: the run was ended; nothing recorded"),
        "{}",
        text(&out)
    );
    let until = Instant::now() + sluice::workers::STOP_GRACE;
    while runs(sleep) {
        assert!(Instant::now() < until, "the check's sleep still runs");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(sessions_running(&s), 0, "the run's slot is held");
    let out = s.sluice(&["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let last = &doc["worktrees"][0]["last_result"];
    assert_eq!(last["outcome"], "stopped", "{doc}");
    support::guard::stop_host_processes(s.state.path());
}

/// resident-workers, "A client killed while its checks run" (the change
/// `sluice-host-runs-a-sessions-checks`, design D4): within the grace of a
/// stopped call no `sleep` of the command runs, and the run's slot is
/// free.
#[test]
fn a_killed_client_ends_its_checks() {
    let s = Setup::new("exec-killed-client");
    let marks = Scratch::new("exec-killed-client-marks");
    let (mut client, pid) = sleeping(&s, &marks, 60);
    let sleep = written_pid(&pid);
    client.kill().unwrap();
    let _ = client.wait();
    let until = Instant::now() + sluice::workers::STOP_GRACE;
    while runs(sleep) {
        assert!(Instant::now() < until, "the check's sleep still runs");
        std::thread::sleep(Duration::from_millis(50));
    }
    eventually("the run's slot is free", || sessions_running(&s) == 0);
    support::guard::stop_host_processes(s.state.path());
}

/// resident-workers, "sluice host stopped while a session's checks run"
/// (the change `sluice-host-runs-a-sessions-checks`, design D4): when
/// `sluice host` has exited on `SIGTERM`, no `sleep` of the command runs,
/// and the client exits 3, saying that sluice's host process ended.
#[test]
fn a_stopped_sluice_host_ends_a_sessions_checks() {
    let s = Setup::new("exec-host-stopped");
    let marks = Scratch::new("exec-host-stopped-marks");
    let (client, pid) = sleeping(&s, &marks, 60);
    let sleep = written_pid(&pid);
    support::guard::stop_host_processes(s.state.path());
    assert!(!runs(sleep), "the check's sleep outlived sluice host");
    let out = client.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(
        text(&out).contains("sluice's host process"),
        "{}",
        text(&out)
    );
}

/// resident-workers, "A worktree's release" (the change
/// `sluice-host-runs-a-sessions-checks`, design D1): a release command
/// that writes a file runs as a worker of `sluice host`, the file is
/// written, and the place is released. The release is a list, so
/// `sluice host` starts the script itself: the script's parent is read
/// after the release ended, and only a parent that still runs has a
/// parent to read. A string runs under `/bin/sh -c`, and dash, the
/// `/bin/sh` of Debian and Ubuntu, starts the script as its own child,
/// which ends with the release.
#[test]
fn a_worktrees_release_runs_in_sluice_host() {
    let s = Setup::new("exec-release-in-host");
    let marks = Scratch::new("exec-release-in-host-marks");
    let parent = marks.path().join("parent");
    let release = marks.path().join("release.sh");
    std::fs::write(&release, format!("echo $PPID > '{}'\n", parent.display())).unwrap();
    s.policy(&POLICY.replace(
        "[tests]\n",
        &format!("[tests]\nrelease = [\"sh\", \"{}\"]\n", release.display()),
    ));
    let out = s.sluice(&["exec", "bolt/x", "--", "true"]);
    assert!(out.status.success(), "{}", text(&out));
    let git = support::CountingGit::new();
    let _held = host_held(&s);
    let client = without_tools(&git, &mut s.command(&["release"]))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = client.id();
    let out = client.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("released "),
        "{}",
        text(&out)
    );
    let parent: u32 = std::fs::read_to_string(&parent)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let chain = ancestors(parent);
    let host = host_pid(&s);
    assert!(
        chain.contains(&host),
        "the release command is not under sluice host {host}: {chain:?}"
    );
    let below = &chain[..chain.iter().position(|p| *p == host).unwrap()];
    assert!(
        !below.contains(&pid),
        "the release command is under sluice release: {chain:?}"
    );
    ran_only_the_upkeep(&git, pid);
}
