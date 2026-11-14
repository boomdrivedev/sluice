//! Scratch directories and repositories for the profiles' acceptance
//! tests.

#![allow(dead_code)]

pub mod http;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// herdr's variables, which every pane it runs sets (51).
pub const HERDR: [&str; 8] = [
    "HERDR_ENV",
    "HERDR_SESSION",
    "HERDR_SOCKET_PATH",
    "HERDR_PANE_ID",
    "HERDR_TAB_ID",
    "HERDR_WORKSPACE_ID",
    "HERDR_AGENT",
    "HERDR_BIN_PATH",
];

/// The directory standing for the user's scratch directory (35 as
/// amended, rule core.30) for every sluice a test starts and every place
/// seam a test builds beside one: `sluice-test-<uid>` in the tests'
/// temporary directory, resolved, shared by every test process of the
/// user as a host's processes share the user's directory, and made by the
/// first place made under it. sluice's gate runs the tests under the
/// sandbox, which lets them write there and not in `/tmp/sluice-<uid>`.
pub fn test_user_scratch() -> PathBuf {
    let tmp = std::env::temp_dir();
    std::fs::canonicalize(&tmp)
        .unwrap_or(tmp)
        .join(format!("sluice-test-{}", nix::unistd::geteuid()))
}

/// Out of reach of whatever runs git under `c`, sluice included: the
/// user's and the system's git config, so their credential helpers, and
/// every prompt and askpass program. No test may reach the user's
/// credentials or ask anyone anything. A sluice run under `c` skips the
/// retry policy's pauses, which the fakes count tries through (134), and
/// is in no herdr pane, so it records none and pushes into none (51).
pub fn isolated(c: &mut Command) -> &mut Command {
    for v in HERDR {
        c.env_remove(v);
    }
    c.env("SLUICE_RETRY_SLEEP", "record")
        .env(
            sluice::profiles::places::USER_SCRATCH_VAR,
            test_user_scratch(),
        )
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
}

/// git, [`isolated`], with every credential helper cleared before any a
/// test gives.
pub fn git_command() -> Command {
    let mut c = Command::new("git");
    isolated(&mut c).args(["-c", "credential.helper="]);
    c
}

/// A repository's own config clears credential helpers and askpass, so
/// the git sluice's profiles run in it reaches none of the user's.
fn no_credentials(dir: &Path) {
    for (key, value) in [("credential.helper", ""), ("core.askPass", "")] {
        let out = git_command()
            .arg("--git-dir")
            .arg(dir)
            .args(["config", key, value])
            .output()
            .expect("run git");
        assert!(out.status.success(), "git config {key}");
    }
}

/// `tests` starting `request` in a combination's part it holds first: a
/// round's start, refused with [`sluice_seams::AtBound`] where the host has
/// no room (71; rule core.49).
pub fn start(
    tests: &dyn sluice_seams::Tests,
    request: &sluice_seams::RunRequest,
) -> anyhow::Result<sluice_seams::RunHandle> {
    let part = tests
        .hold(sluice_seams::RunKind::Queue)?
        .ok_or(sluice_seams::AtBound)?;
    tests.start(request, part)
}

/// Whether `free` holds within two seconds. A lock a process let go may
/// still be held for a moment by a process another thread of this binary
/// starts meanwhile, until it runs its program: a check that a lock is
/// free allows that moment, and a check that one is held allows nothing
/// (129). The check's own try of the lock is such a letting go, so it
/// gives that a moment too before it answers.
pub fn soon(mut free: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if free() {
            std::thread::sleep(std::time::Duration::from_millis(10));
            return true;
        }
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A directory removed when the test ends.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sluice-test-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(std::fs::canonicalize(&dir).expect("canonical scratch dir"))
    }

    /// A scratch directory with as short a path as the host's temporary
    /// directory allows, for a socket: its path is at most 104 bytes on
    /// macOS, and a run's temporary directory under `sluice exec` is long.
    pub fn short() -> Scratch {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "s{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(std::fs::canonicalize(&dir).expect("canonical scratch dir"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A git that counts its starts (129): a directory holding a `git` script
/// that appends the pid of the process that started it and its arguments,
/// one line per start, to a log, then runs the
/// git found on the path when it was made, by its absolute path. A test
/// puts the directory first on the path of the sluice it starts, and
/// every other isolation stays.
pub struct CountingGit {
    scratch: Scratch,
}

impl CountingGit {
    pub fn new() -> CountingGit {
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|d| d.join("git"))
            .find(|p| p.is_file())
            .expect("git on the path");
        let scratch = Scratch::new("counting-git");
        let bin = scratch.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = bin.join("git");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\t%s\\n' \"$PPID\" \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
                scratch.path().join("log").display(),
                real.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        CountingGit { scratch }
    }

    /// `c` with this git first on its path: the path `c` was given, or
    /// the test's own.
    pub fn on<'a>(&self, c: &'a mut Command) -> &'a mut Command {
        let given = c
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v.map(|v| v.to_os_string()));
        let rest = given
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        let mut path = vec![self.scratch.path().join("bin")];
        path.extend(std::env::split_paths(&rest));
        c.env("PATH", std::env::join_paths(path).unwrap())
    }

    /// Each start so far, its arguments as one line.
    pub fn starts(&self) -> Vec<String> {
        self.parented().into_iter().map(|(_, args)| args).collect()
    }

    /// Each start so far: the pid of the process that started it, and its
    /// arguments as one line.
    pub fn parented(&self) -> Vec<(u32, String)> {
        std::fs::read_to_string(self.scratch.path().join("log"))
            .unwrap_or_default()
            .lines()
            .map(|l| match l.split_once('\t') {
                Some((pid, args)) => (pid.parse().unwrap_or(0), args.to_string()),
                None => (0, l.to_string()),
            })
            .collect()
    }

    /// How many times it has started.
    pub fn count(&self) -> usize {
        self.starts().len()
    }
}

/// A repository on disk whose `main` holds a policy, driven through the
/// binary with the host's config and state its own and git counting its
/// starts (129).
pub struct OnDisk {
    pub repo: Repo,
    pub git: CountingGit,
    pub config: Scratch,
    pub state: Scratch,
    pub sessions: Scratch,
}

impl OnDisk {
    /// `policy` committed on `main`, the branch checked out.
    pub fn new(name: &str, policy: &str) -> OnDisk {
        let repo = Repo::new(name);
        repo.write(".config/sluice.toml", policy);
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        OnDisk {
            repo,
            git: CountingGit::new(),
            config: Scratch::new(&format!("{name}-config")),
            state: Scratch::new(&format!("{name}-state")),
            sessions: Scratch::new(&format!("{name}-sessions")),
        }
    }

    /// The binary, run in `dir` under [`isolated`], with this setup's
    /// config, state and counting git, in no session.
    pub fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
        isolated(&mut c)
            .current_dir(dir)
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

    /// The binary run in the repository with `args`, its pauses between
    /// rounds skipped.
    pub fn sluice(&self, args: &[&str]) -> std::process::Output {
        self.command(self.repo.dir())
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(args)
            .output()
            .unwrap()
    }

    /// A session's worktree on `unit/<name>` off `main`, with one commit
    /// of its own file: its directory.
    pub fn session(&self, name: &str) -> PathBuf {
        self.session_with(name, &[])
    }

    /// [`OnDisk::session`], its commit also writing `files`.
    pub fn session_with(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.sessions.path().join(name);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            "main",
        ]);
        std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
        for (path, content) in files {
            std::fs::write(dir.join(path), content).unwrap();
        }
        let at = dir.to_str().unwrap();
        self.repo.git(&["-C", at, "add", "-A"]);
        self.repo.git(&["-C", at, "commit", "-q", "-m", name]);
        dir
    }

    /// [`OnDisk::session`] submitted to `main` and waited on until it
    /// landed.
    pub fn land(&self, name: &str) {
        self.land_with(name, &[]);
    }

    /// [`OnDisk::session_with`] submitted to `main` and waited on until it
    /// landed.
    pub fn land_with(&self, name: &str, files: &[(&str, &str)]) {
        let dir = self.session_with(name, files);
        let out = self
            .command(&dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["submit", "main"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The environment `c` runs with: the test's own, with what `c` sets and
/// removes.
pub fn env_of(c: &Command) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
        std::env::vars_os().collect();
    for (k, v) in c.get_envs() {
        match v {
            Some(v) => {
                env.insert(k.to_os_string(), v.to_os_string());
            }
            None => {
                env.remove(k);
            }
        }
    }
    env.into_iter().collect()
}

impl OnDisk {
    /// The way to this setup's host process, as a command run in `dir`
    /// with this setup's environment reaches it (135).
    pub fn client(&self, dir: &Path) -> sluice::host_process::client::Client {
        sluice::host_process::client::Client::new(
            Path::new(env!("CARGO_BIN_EXE_sluice")),
            env_of(&self.command(dir)),
        )
        .expect("the host process's paths")
    }

    /// A second repository on this host, its `main` holding `policy`,
    /// driven with this setup's config and state.
    pub fn another(&self, name: &str, policy: &str) -> Repo {
        let repo = Repo::new(name);
        repo.write(".config/sluice.toml", policy);
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        repo
    }

    /// The repository holding `dir`, as sluice names it: its common git
    /// directory, as the overview's `repository` gives it.
    pub fn repository(&self, dir: &Path) -> String {
        let out = self
            .command(dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["overview", "--json"])
            .output()
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
        doc["repository"]
            .as_str()
            .expect("a repository")
            .to_string()
    }
}

/// A repository with one worktree, built with the git command.
pub struct Repo {
    pub scratch: Scratch,
}

/// What a place sluice made in the repository keeps outside it, its
/// directory under the user's scratch directory (35 as amended, rule
/// core.30), the tests' own and the user's, goes with it.
impl Drop for Repo {
    fn drop(&mut self) {
        let Ok(common) = std::fs::canonicalize(self.dir().join(".git")) else {
            return;
        };
        let key = sluice::profiles::places::repository_key(&common);
        let _ = std::fs::remove_dir_all(test_user_scratch().join(key));
        if let Ok(dir) = sluice::profiles::places::repository_scratch(&common) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Repo {
    pub fn new(name: &str) -> Repo {
        let repo = Repo {
            scratch: Scratch::new(name),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        no_credentials(&repo.dir().join(".git"));
        repo.write("README", "scratch\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "root"]);
        repo
    }

    pub fn dir(&self) -> &Path {
        self.scratch.path()
    }

    /// A bare clone of this repository, added as its remote `origin`: a
    /// remote to git as a forge is.
    pub fn bare_origin(&self) -> Scratch {
        let bare = Scratch::new("origin.git");
        let at = bare.path().to_str().unwrap();
        self.git(&["clone", "-q", "--bare", ".", at]);
        no_credentials(bare.path());
        self.git(&["remote", "add", "origin", at]);
        self.git(&["fetch", "-q", "origin"]);
        bare
    }

    /// A clone of `remote`, as another host's repository.
    pub fn clone_of(remote: &Path, name: &str) -> Repo {
        let repo = Repo {
            scratch: Scratch::new(name),
        };
        repo.git(&["clone", "-q", remote.to_str().unwrap(), "."]);
        no_credentials(&repo.dir().join(".git"));
        repo
    }

    /// Commit `changes` on the branch checked out and push it to `origin`;
    /// returns the new head.
    pub fn push_change(&self, path: &str, content: &str) -> String {
        self.write(path, content);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", path]);
        self.git(&["push", "-q", "origin", "HEAD:main"]);
        self.rev("HEAD")
    }

    /// Run git in the worktree; panics on failure, returns stdout trimmed.
    pub fn git(&self, args: &[&str]) -> String {
        let out = self.git_raw(args);
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub fn git_raw(&self, args: &[&str]) -> std::process::Output {
        git_command()
            .current_dir(self.dir())
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("XDG_CONFIG_HOME", self.dir().join(".git/no-config"))
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@localhost")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@localhost")
            .stdin(Stdio::null())
            .output()
            .expect("run git")
    }

    pub fn write(&self, path: &str, content: &str) {
        let p = self.dir().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn rev(&self, name: &str) -> String {
        self.git(&["rev-parse", name])
    }

    /// A branch off `from` with one commit applying `changes` (`None`
    /// deletes, `Some((from, to))` in `renames` renames); returns its head.
    pub fn branch(&self, name: &str, from: &str, changes: &[(&str, Option<&str>)]) -> String {
        self.branch_with(name, from, changes, &[])
    }

    pub fn branch_with(
        &self,
        name: &str,
        from: &str,
        changes: &[(&str, Option<&str>)],
        renames: &[(&str, &str)],
    ) -> String {
        self.git(&["switch", "-q", "-C", name, from]);
        for (path, content) in changes {
            match content {
                Some(c) => self.write(path, c),
                None => {
                    self.git(&["rm", "-q", path]);
                }
            }
        }
        for (a, b) in renames {
            self.git(&["mv", a, b]);
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", name]);
        let head = self.rev("HEAD");
        self.git(&["switch", "-q", "--detach", "main"]);
        head
    }
}

thread_local! {
    /// This thread's readers directory, made on first use, or the one
    /// [`share_readers`] gave it.
    static READERS: RefCell<Option<(PathBuf, Option<Scratch>)>> = const { RefCell::new(None) };
    /// The version of sluice this thread's fakes' host is (129).
    static VERSION: Cell<&'static str> = const { Cell::new(sluice::VERSION) };
}

/// The version of sluice the fakes' host is on this thread: `VERSION`,
/// unless [`set_version`] gave it another.
pub fn version() -> &'static str {
    VERSION.with(Cell::get)
}

/// Make the fakes' host on this thread a sluice of `version`, as a
/// process of a newer or older sluice than a reader the test holds.
pub fn set_version(version: &'static str) {
    VERSION.with(|v| v.set(version));
}

/// The fakes' host's readers directory (129), the test's own: each test
/// runs on a thread of its own, and so finds no reader but its own.
pub fn readers() -> PathBuf {
    READERS.with(|r| {
        let mut r = r.borrow_mut();
        let (dir, _) = r.get_or_insert_with(|| {
            let own = Scratch::new("readers");
            (own.path().to_path_buf(), Some(own))
        });
        dir.clone()
    })
}

/// Give this thread the readers directory `dir`: a thread of the test's
/// that is another process on the same host.
pub fn share_readers(dir: PathBuf) {
    READERS.with(|r| *r.borrow_mut() = Some((dir, None)));
}

/// The host over the fakes: session runs counted by the fake check seam,
/// a reserve, no run directories, the test's own [`readers`], the thread's
/// [`version`], and the host process in the test's own process.
pub struct FakeHost {
    pub world: sluice_fakes::World,
    pub reserved: usize,
    pub process: FakeProcess,
}

impl FakeHost {
    pub fn new(world: sluice_fakes::World, reserved: usize) -> FakeHost {
        FakeHost {
            world,
            reserved,
            process: FakeProcess {
                reserved,
                ..FakeProcess::default()
            },
        }
    }
}

thread_local! {
    /// Whether the fakes' host process on this thread is to end at its
    /// next ask, as one killed does.
    static ENDS: Cell<bool> = const { Cell::new(false) };
    /// The room of the fakes' host process on this thread, which outlives
    /// each command a test runs, as sluice's host process outlives the
    /// commands that reach it (record 0087; design D12).
    static ROOM: RefCell<Option<sluice::host_process::room::Shared>> = const { RefCell::new(None) };
}

/// The host process the overview `c` runs reaches, where one answers,
/// killed and waited for: one keeps the settings it read until it ends,
/// so the next command starts one from its own environment that reads the
/// host's configuration as it then stands (135; rule core.51; design D13).
pub fn kill_host_process(c: &mut Command) {
    let out = c.args(["overview", "--json"]).output().unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let Some(pid) = doc["host_process"]["pid"].as_i64() else {
        return;
    };
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while nix::sys::signal::kill(pid, None).is_ok() {
        assert!(
            std::time::Instant::now() < until,
            "the host process lives on"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The fakes' host process on this thread ends at its next ask, killed:
/// it lets the reader's place go, answers that ask with nothing, and the
/// next ask finds one started again, which takes the place afresh (129).
pub fn end_host_process() {
    ENDS.with(|e| e.set(true));
}

/// The fakes' host's room on this thread, bounded as the check seam and
/// `reserved` say now.
pub fn fake_room(bound: usize, reserved: usize) -> sluice::host_process::room::Shared {
    ROOM.with(|r| {
        let room = r
            .borrow_mut()
            .get_or_insert_with(|| {
                sluice::host_process::room::Shared::new(sluice::host_process::room::Room::seeing(
                    bound, reserved,
                ))
            })
            .clone();
        room.lock().bounds(bound, reserved);
        room
    })
}

/// sluice's host process as the fakes' host answers it, in the test's own
/// process (design D13): a pass and a step run the pass or the round on
/// the fakes' seams there, through the same `host_process::pass` the host
/// process runs; a session's or a preview's room is asked of the room it
/// keeps; and it passes only when asked, so a follower asks a pass at each
/// tick and a serve holds the repository for the next ask. What it reads
/// with lasts one command.
#[derive(Default)]
pub struct FakeProcess {
    reserved: usize,
    kept: RefCell<sluice::host_process::pass::Kept>,
    /// The policies its passes read, kept across them as the host
    /// process's task keeps them (129).
    policies: sluice_core::Policies,
    reader: RefCell<Option<sluice::reader::Reader>>,
    driver: RefCell<Option<sluice::reader::Driver>>,
}

impl FakeProcess {
    fn room(
        &self,
        here: &sluice::host_process::Here<'_, '_>,
    ) -> anyhow::Result<sluice::host_process::room::Shared> {
        Ok(fake_room(here.s.seams().tests.bound()?, self.reserved))
    }

    /// The fakes' seams with their parts held of the room, as the host
    /// process holds a repository's.
    fn roomed<'a>(
        &self,
        here: &sluice::host_process::Here<'a, '_>,
    ) -> anyhow::Result<sluice::host_process::room::Roomed<'a>> {
        Ok(sluice::host_process::room::Roomed {
            inner: here.s.seams().tests,
            room: self.room(here)?,
            repository: here.repository()?,
        })
    }
}

impl sluice::host_process::HostProcess for FakeProcess {
    fn asked_only(&self) -> bool {
        true
    }

    fn note(&self, _: &sluice::host_process::Here<'_, '_>) -> anyhow::Result<()> {
        Ok(())
    }

    /// It passes only when asked: a serve holds the repository, and the
    /// test's next command that asks moves it.
    fn serve(&self, _: &sluice::host_process::Here<'_, '_>) -> anyhow::Result<()> {
        Ok(())
    }

    /// The fakes' host process is the test's own process: it answers.
    fn reach(&self, _: &sluice::host_process::Here<'_, '_>) -> anyhow::Result<()> {
        Ok(())
    }

    fn step(
        &self,
        here: &sluice::host_process::Here<'_, '_>,
        branch: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let roomed = self.roomed(here)?;
        let seams = sluice_core::Seams {
            tests: &roomed,
            ..here.s.seams()
        };
        let s = sluice_core::Sluice::new(seams, here.clock)
            .as_peer(here.host.peer())
            .keeping(&self.policies);
        sluice::host_process::pass::step(&s, here.pulls, here.clock, branch)
    }

    fn pass(
        &self,
        here: &sluice::host_process::Here<'_, '_>,
    ) -> anyhow::Result<sluice::host_process::pass::Report> {
        use sluice::reader::{Driver, Drives, Reader};
        if ENDS.with(|e| e.replace(false)) {
            *self.reader.borrow_mut() = None;
            *self.driver.borrow_mut() = None;
            *self.kept.borrow_mut() = Default::default();
            anyhow::bail!("sluice's host process ended");
        }
        let roomed = self.roomed(here)?;
        let seams = sluice_core::Seams {
            tests: &roomed,
            ..here.s.seams()
        };
        let s = sluice_core::Sluice::new(seams, here.clock)
            .as_peer(here.host.peer())
            .keeping(&self.policies);
        let (readers, repository) = (here.host.readers()?, here.host.repository()?);
        let mut driver = self.driver.borrow_mut();
        if driver.is_none() {
            *driver = Driver::try_take(&readers, &repository)?;
        }
        // Design D7: an older `sluice run` drives the repository.
        if driver.is_none() {
            return Ok(sluice::host_process::pass::Report {
                in_play: true,
                ..Default::default()
            });
        }
        let mut reader = self.reader.borrow_mut();
        match &mut *reader {
            Some(r) => {
                let _ = r.step(true, Drives::Nobody);
            }
            None => {
                let mut r =
                    Reader::take_given(&readers, &repository, here.host.version(), Drives::Nobody)?;
                // An older reader's lease, taken at once.
                let _ = r.step(true, Drives::Nobody);
                *reader = Some(r);
            }
        }
        // Design D7: an older reader reads until it lets go.
        let Some(Reader::Lead(lead)) = &mut *reader else {
            return Ok(sluice::host_process::pass::Report {
                in_play: true,
                ..Default::default()
            });
        };
        sluice::host_process::pass::pass(
            &s,
            here.host,
            here.pulls,
            here.clock,
            Some(lead),
            sluice::tick(None),
            &mut self.kept.borrow_mut(),
        )
    }

    /// It passes only when asked: a read is answered at once, as the
    /// fakes' records stand, which every read reads directly.
    fn read(
        &self,
        here: &sluice::host_process::Here<'_, '_>,
    ) -> anyhow::Result<sluice::host_process::protocol::Reading> {
        Ok(sluice::host_process::protocol::Reading {
            current: true,
            ended: Some(
                here.clock
                    .now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
            refused: None,
        })
    }

    fn follow<'a>(
        &'a self,
        _: &sluice::host_process::Here<'a, '_>,
        _: sluice::host_process::protocol::FollowKind,
    ) -> anyhow::Result<Box<dyn sluice::host_process::Following + 'a>> {
        Ok(Box::new(NoReports))
    }

    fn hold<'a>(
        &'a self,
        here: &sluice::host_process::Here<'a, '_>,
        kind: sluice_seams::RunKind,
        _place: &str,
    ) -> anyhow::Result<Box<dyn sluice::host_process::Holding + 'a>> {
        let roomed = self.roomed(here)?;
        let waiter = roomed.room.lock().wait(&roomed.repository);
        Ok(Box::new(FakeHolding {
            roomed,
            waiter,
            kind,
            said: None,
        }))
    }

    fn room(
        &self,
        here: &sluice::host_process::Here<'_, '_>,
    ) -> anyhow::Result<
        Option<(
            sluice::host_process::protocol::Named,
            sluice::host_process::protocol::RoomView,
        )>,
    > {
        let roomed = self.roomed(here)?;
        let running = sluice_seams::Tests::running(&roomed)?;
        let mut room = roomed.room.lock();
        let now = std::time::Instant::now();
        let view = sluice::host_process::protocol::RoomView {
            bound: room.bound(),
            reserve: room.reserved(),
            queue: running.queue,
            sessions: running.sessions,
            waiting: room.waiting(now),
            turn: room.turn(),
            repositories: room.repositories(now)?,
        };
        let named = sluice::host_process::protocol::Named {
            version: here.host.version().to_string(),
            pid: std::process::id(),
            since: "2026-11-07T00:00:00Z".into(),
            asked_only: true,
        };
        Ok(Some((named, view)))
    }
}

/// A follow of the fakes' host process, which reports nothing of its own:
/// its follower asks a pass at each tick.
struct NoReports;

impl sluice::host_process::Following for NoReports {
    fn next(
        &mut self,
        _: std::time::Duration,
    ) -> anyhow::Result<Option<sluice::host_process::pass::Report>> {
        Ok(None)
    }
}

/// A session's or a preview's ask of the fakes' room, its wait let go
/// when it is dropped.
struct FakeHolding<'a> {
    roomed: sluice::host_process::room::Roomed<'a>,
    waiter: u64,
    kind: sluice_seams::RunKind,
    said: Option<sluice::host_process::room::Why>,
}

impl sluice::host_process::Holding for FakeHolding<'_> {
    fn next(
        &mut self,
        _: std::time::Duration,
    ) -> anyhow::Result<Option<sluice::host_process::Holds>> {
        use sluice::host_process::room::{Asked, Asker};
        match self.roomed.ask(&Asker::Session(self.waiter))? {
            Asked::Held(id) => Ok(Some(sluice::host_process::Holds::Held(
                self.roomed.room.part(self.kind, id),
            ))),
            Asked::Refused(why) if self.said != Some(why) => {
                self.said = Some(why);
                Ok(Some(sluice::host_process::Holds::Waiting(why)))
            }
            Asked::Refused(_) => Ok(None),
        }
    }
}

impl Drop for FakeHolding<'_> {
    fn drop(&mut self) {
        self.roomed.room.lock().let_go(self.waiter);
    }
}

impl sluice::exec::Host for FakeHost {
    fn reserved(&self) -> usize {
        self.reserved
    }

    fn host_process(&self) -> &dyn sluice::host_process::HostProcess {
        &self.process
    }

    fn worktree(&self, dir: &str) -> anyhow::Result<String> {
        Ok(dir.to_string())
    }

    fn lock_sessions(&self) -> anyhow::Result<Box<dyn std::any::Any>> {
        Ok(Box::new(()))
    }

    fn begin_session(
        &self,
        _: &dyn std::any::Any,
        run: &sluice::sessions::SessionRun,
    ) -> anyhow::Result<()> {
        self.world.begin_host_run(sluice_seams::HostRun {
            worktree: run.worktree.clone(),
            branch: run.branch.clone(),
            started: run.started.clone(),
            command: run.command.clone(),
            tasks: run.tasks.clone(),
        });
        Ok(())
    }

    fn end_session(&self, worktree: &str) -> anyhow::Result<()> {
        self.world.kill_session(worktree);
        Ok(())
    }

    fn session_tasks(
        &self,
        worktree: &str,
        tasks: &[sluice_seams::TaskProgress],
    ) -> anyhow::Result<()> {
        self.world.session_tasks(worktree, tasks);
        Ok(())
    }

    fn run_dir(&self, _: &sluice_seams::RunHandle) -> Option<PathBuf> {
        None
    }

    fn end_run(&self, _: &sluice_seams::RunHandle) -> anyhow::Result<()> {
        Ok(())
    }

    /// The fakes' revisions are named whole.
    fn revision(&self, rev: &str) -> anyhow::Result<Option<String>> {
        Ok(Some(rev.to_string()))
    }

    fn name(&self) -> String {
        "fake-host".into()
    }

    fn repository(&self) -> anyhow::Result<String> {
        Ok("/fake/repo.git".into())
    }

    /// The fakes' repository is no directory: it is gone when the world
    /// says so.
    fn repository_gone(&self, _: &str) -> bool {
        self.world.is_repository_gone()
    }

    fn tips(&self) -> anyhow::Result<Vec<sluice_seams::Rev>> {
        self.world.tips()
    }

    /// The fakes hold every tip [`FakeHost::tips`] reads.
    fn holds_policy(&self, _tips: &[sluice_seams::Rev]) -> anyhow::Result<bool> {
        self.world.holds_path(sluice_core::policy::POLICY_PATH)
    }

    fn readers(&self) -> anyhow::Result<PathBuf> {
        Ok(readers())
    }

    fn version(&self) -> &str {
        version()
    }

    fn default_branch(&self) -> anyhow::Result<Option<sluice_core::overview::OrderFrom>> {
        Ok(self
            .world
            .head()
            .map(|(branch, local)| sluice_core::overview::OrderFrom { branch, local }))
    }
}

/// A fake container engine listening at `e.sock` in `dir`, answering a
/// `GET /_ping` with `status` and anything else with a 404; its socket's
/// path. Make `dir` with [`Scratch::short`]: a socket's path is at most
/// 104 bytes on macOS.
pub fn engine(dir: &Path, status: &'static str) -> PathBuf {
    let path = dir.join("e.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            answer_ping(&mut c, status);
        }
    });
    path
}

/// A fake container engine's answer on `c`: `status` to a `GET /_ping`,
/// a 404 to anything else.
fn answer_ping(c: &mut (impl std::io::Read + std::io::Write), status: &str) {
    let mut asked = Vec::new();
    let mut buf = [0u8; 512];
    while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
        match c.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => asked.extend_from_slice(&buf[..n]),
        }
    }
    let status = match asked.starts_with(b"GET /_ping ") {
        true => status,
        false => "HTTP/1.0 404 Not Found",
    };
    let _ = write!(
        c,
        "{status}\r\nApi-Version: 1.47\r\nContent-Length: 2\r\n\r\nOK"
    );
    let _ = c.flush();
}

/// A fake container engine on a local TCP port, answering a `GET /_ping`
/// with `status` and anything else with a 404 (design D5): its port.
pub fn tcp_engine(status: &'static str) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            answer_ping(&mut c, status);
        }
    });
    port
}

/// A local TCP port nothing listens on, as far as the test can tell: bound
/// and let go, so the kernel may hand it to the next bind on the host
/// before the test connects.
pub fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// What `probe` makes of a local port nothing listens on: the first of a
/// few such ports at which `refused` holds of it, or else the last tried.
/// A port let go may be bound by another socket on the host before the
/// probe connects, which then answers in the refusal's place (the queue's
/// cli:test, 2026-11-05: a test's TLS engine beside this one resetting the
/// connection); a port held bound cannot stand in, since macOS drops a
/// connection to it where Linux refuses it.
pub fn at_a_closed_port<T>(
    mut probe: impl FnMut(u16) -> T,
    refused: impl Fn(u16, &T) -> bool,
) -> (u16, T) {
    let mut tried = None;
    for _ in 0..5 {
        let port = closed_port();
        let seen = probe(port);
        if refused(port, &seen) {
            return (port, seen);
        }
        tried = Some((port, seen));
    }
    tried.unwrap()
}

/// What `probe` makes of a Unix socket in `dir` that nothing listens on:
/// the first of a few such sockets, each bound and let go, at which
/// `refused` holds of it, or else the last tried. On macOS a socket is
/// made close-on-exec only after it is made, so a child another test
/// spawns between the two keeps it listening for the child's life, and a
/// connection to it is taken and dropped in the refusal's place (the
/// queue's cli:test, 2026-11-05: `closed the connection unanswered`).
pub fn at_a_closed_socket<T>(
    dir: &Path,
    mut probe: impl FnMut(&Path) -> T,
    refused: impl Fn(&Path, &T) -> bool,
) -> (PathBuf, T) {
    let mut tried = None;
    for n in 0..5 {
        let socket = dir.join(format!("e{n}.sock"));
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        let seen = probe(&socket);
        if refused(&socket, &seen) {
            return (socket, seen);
        }
        tried = Some((socket, seen));
    }
    tried.unwrap()
}

/// A fake container engine over TLS on a local TCP port, answering `200`
/// to a `GET /_ping` only from a client whose certificate its own CA
/// signed (design D5). The CA's certificate, and a client certificate and
/// key it signed, are written to `dir` as `ca.pem`, `cert.pem` and
/// `key.pem`, as `DOCKER_CERT_PATH` holds them: its port.
pub fn tls_engine(dir: &Path) -> u16 {
    use std::sync::Arc;

    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair, KeyUsagePurpose,
    };
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca = CertifiedIssuer::self_signed(ca, KeyPair::generate().unwrap()).unwrap();
    let signed = |names: Vec<String>, usage| {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(names).unwrap();
        params.extended_key_usages = vec![usage];
        (params.signed_by(&key, &ca).unwrap(), key)
    };
    let (server, server_key) = signed(
        vec!["127.0.0.1".into(), "localhost".into()],
        ExtendedKeyUsagePurpose::ServerAuth,
    );
    let (client, client_key) = signed(Vec::new(), ExtendedKeyUsagePurpose::ClientAuth);
    std::fs::write(dir.join("ca.pem"), ca.pem()).unwrap();
    std::fs::write(dir.join("cert.pem"), client.pem()).unwrap();
    std::fs::write(dir.join("key.pem"), client_key.serialize_pem()).unwrap();

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der()));
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![server.der().clone()], key)
            .unwrap(),
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(tcp) = conn else { continue };
            let Ok(tls) = rustls::ServerConnection::new(config.clone()) else {
                continue;
            };
            let mut c = rustls::StreamOwned::new(tls, tcp);
            answer_ping(&mut c, "HTTP/1.0 200 OK");
            c.conn.send_close_notify();
            let _ = std::io::Write::flush(&mut c);
        }
    });
    port
}

/// What [`Engine::refuse`] has the fake engine answer a removal it
/// refuses: its status and its `message`.
pub const REFUSED: (&str, &str) = (
    "500 Internal Server Error",
    "could not kill the container: the fake engine refuses it",
);

/// The containers a fake [`Engine`] holds, made at each listing.
type Held = Box<dyn Fn() -> serde_json::Value + Send>;

/// What a fake [`Engine`] holds and was asked.
struct EngineState {
    held: Held,
    /// The ids removed, in order: no longer listed.
    removed: Vec<String>,
    /// The removals still to refuse, by id.
    refuse: std::collections::BTreeMap<String, u32>,
    /// How many times its containers were listed.
    listings: u32,
    /// Whether it answers as an engine that keeps each connection open.
    open: bool,
}

/// A fake container engine (record 0121): `GET /_ping` answers `200`;
/// `GET /containers/json?all=true` the containers it holds, less those
/// removed; `DELETE /containers/<id>?force=true&v=true` removes one and
/// answers `204`, `404` for one it does not hold, or [`REFUSED`] while
/// [`Engine::refuse`] says; any other ask a `404`, or a `400` for a
/// removal without both of its parameters. Each connection is answered
/// as the engine answers HTTP/1.0, then closed, unless
/// [`Engine::keep_open`] says otherwise.
#[derive(Clone)]
pub struct Engine {
    socket: Option<PathBuf>,
    port: Option<u16>,
    state: std::sync::Arc<std::sync::Mutex<EngineState>>,
}

impl Engine {
    fn new() -> Engine {
        Engine {
            socket: None,
            port: None,
            state: std::sync::Arc::new(std::sync::Mutex::new(EngineState {
                held: Box::new(|| serde_json::json!([])),
                removed: Vec::new(),
                refuse: Default::default(),
                listings: 0,
                open: false,
            })),
        }
    }

    /// Hold `containers`, a list as `GET /containers/json` gives one.
    pub fn hold(&self, containers: serde_json::Value) -> &Engine {
        self.hold_with(move || containers.clone())
    }

    /// Hold the containers `make` makes, at each listing: for those that
    /// depend on what a run wrote.
    pub fn hold_with(&self, make: impl Fn() -> serde_json::Value + Send + 'static) -> &Engine {
        self.state.lock().unwrap().held = Box::new(make);
        self
    }

    /// Answer as one host's engine answers (2026-11-11): in HTTP/1.1
    /// whatever was asked, a listing in chunks, a removal's `204` with no
    /// length, and each connection held open after its answer until the
    /// asker closes it.
    pub fn keep_open(&self) -> &Engine {
        self.state.lock().unwrap().open = true;
        self
    }

    /// Refuse the next `times` removals of `id` with [`REFUSED`].
    pub fn refuse(&self, id: &str, times: u32) -> &Engine {
        self.state
            .lock()
            .unwrap()
            .refuse
            .insert(id.to_string(), times);
        self
    }

    /// The ids removed, in order.
    pub fn removed(&self) -> Vec<String> {
        self.state.lock().unwrap().removed.clone()
    }

    /// How many times its containers were listed.
    pub fn listings(&self) -> u32 {
        self.state.lock().unwrap().listings
    }

    /// Its unix socket's path.
    pub fn socket(&self) -> PathBuf {
        self.socket.clone().expect("an engine on a unix socket")
    }

    /// Its local TCP port.
    pub fn port(&self) -> u16 {
        self.port.expect("an engine on a TCP port")
    }

    /// Answer one connection `c`.
    fn answer(&self, c: &mut (impl std::io::Read + std::io::Write)) {
        let mut asked = Vec::new();
        let mut buf = [0u8; 512];
        while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
            match c.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => asked.extend_from_slice(&buf[..n]),
            }
        }
        let line = String::from_utf8_lossy(&asked);
        let mut words = line.split_whitespace();
        let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        let message = |m: &str| serde_json::json!({ "message": m }).to_string();
        let (status, body) = match (method, target.split_once('?')) {
            ("GET", _) if target == "/_ping" => ("200 OK".to_string(), "OK".to_string()),
            ("GET", Some(("/containers/json", "all=true"))) => {
                let mut s = self.state.lock().unwrap();
                s.listings += 1;
                let held = (s.held)();
                let listed: Vec<_> = held
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| !s.removed.iter().any(|r| c["Id"] == r.as_str()))
                    .cloned()
                    .collect();
                ("200 OK".into(), serde_json::Value::from(listed).to_string())
            }
            ("DELETE", Some((path, query))) if path.starts_with("/containers/") => {
                let id = &path["/containers/".len()..];
                let mut s = self.state.lock().unwrap();
                if query != "force=true&v=true" {
                    ("400 Bad Request".into(), message(&format!("asked {query}")))
                } else if s.refuse.get(id).is_some_and(|n| *n > 0) {
                    *s.refuse.get_mut(id).unwrap() -= 1;
                    (REFUSED.0.into(), message(REFUSED.1))
                } else {
                    let held = (s.held)();
                    let holds = held.as_array().into_iter().flatten().any(|c| c["Id"] == id)
                        && !s.removed.iter().any(|r| r == id);
                    match holds {
                        true => {
                            s.removed.push(id.to_string());
                            ("204 No Content".into(), String::new())
                        }
                        false => (
                            "404 Not Found".into(),
                            message(&format!("No such container: {id}")),
                        ),
                    }
                }
            }
            _ => ("404 Not Found".into(), message("page not found")),
        };
        if !self.state.lock().unwrap().open {
            let _ = write!(
                c,
                "HTTP/1.0 {status}\r\nApi-Version: 1.47\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = c.flush();
            return;
        }
        let head = format!("HTTP/1.1 {status}\r\nApi-Version: 1.47\r\n");
        let _ = match status.starts_with("204 ") {
            true => write!(c, "{head}\r\n"),
            false => {
                let chunks: String = body
                    .as_bytes()
                    .chunks(7)
                    .map(|b| format!("{:x}\r\n{}\r\n", b.len(), String::from_utf8_lossy(b)))
                    .collect();
                write!(
                    c,
                    "{head}Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{chunks}0\r\n\r\n"
                )
            }
        };
        let _ = c.flush();
        // Held until the asker closes it.
        while matches!(c.read(&mut buf), Ok(n) if n > 0) {}
    }
}

/// A fake [`Engine`] at `e.sock` in `dir`. Make `dir` with
/// [`Scratch::short`]: a socket's path is at most 104 bytes on macOS.
pub fn containers_engine(dir: &Path) -> Engine {
    let path = dir.join("e.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let engine = Engine {
        socket: Some(path),
        ..Engine::new()
    };
    let serving = engine.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            serving.answer(&mut c);
        }
    });
    engine
}

/// A fake [`Engine`] on a local TCP port.
pub fn tcp_containers_engine() -> Engine {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let engine = Engine {
        port: Some(listener.local_addr().unwrap().port()),
        ..Engine::new()
    };
    let serving = engine.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            serving.answer(&mut c);
        }
    });
    engine
}

/// A container as the engine lists it: `id`, named `/<id>`, mounting each
/// of `mounts`, publishing each of `ports` on the host, and labelled with
/// each of `labels`.
pub fn container(
    id: &str,
    mounts: &[&Path],
    ports: &[u16],
    labels: &[(&str, &str)],
) -> serde_json::Value {
    serde_json::json!({
        "Id": id,
        "Names": [format!("/{id}")],
        "Image": "fake",
        "Labels": labels.iter().map(|(k, v)| (k.to_string(), serde_json::Value::from(*v))).collect::<serde_json::Map<_, _>>(),
        "Ports": ports.iter().map(|p| serde_json::json!({"IP": "0.0.0.0", "PrivatePort": 80, "PublicPort": p, "Type": "tcp"})).collect::<Vec<_>>(),
        "Mounts": mounts.iter().map(|m| serde_json::json!({"Type": "bind", "Source": m, "Destination": "/mnt", "RW": true})).collect::<Vec<_>>(),
    })
}
