//! S72 on the binary (147): a session's command, a preview's command and
//! `sluice equip` in a worktree sluice did not make, each equipped by the
//! check profile's one sequence on a scratch repository whose pins
//! nothing has installed. Stand-ins for proto and moon log each call; the
//! policy's toolchain, outside the repository, puts them on the path, and
//! the stand-in `moon` is there only once the stand-in `proto install`
//! has written it. Needs git's worktrees, so it is in the check-tools
//! tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use support::{Repo, Scratch};

/// What every working copy a command runs in is equipped by, in order.
const EQUIPPED: [&str; 4] = [
    "toolchain",
    "proto install --config-mode upwards",
    "moon --version",
    "moon setup",
];

struct Setup {
    repo: Repo,
    /// The stand-ins, the toolchain and the logs they write.
    tools: Scratch,
    config: Scratch,
    state: Scratch,
    worktrees: Scratch,
    /// The fake container engine every `sluice` here is pointed at, so a
    /// preview's release ends its containers there and never at the
    /// host's (record 0121).
    engine: support::Engine,
    _engine_dir: Scratch,
}

impl Setup {
    /// `main` holding a `.prototools`, a policy naming the moon runner and
    /// the test's toolchain, and a preview whose command is `moon run
    /// app:preview`; no moon anywhere until `proto install` writes one.
    fn new(name: &str) -> Setup {
        let tools = Scratch::new(&format!("{name}-tools"));
        let bin = tools.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = tools.path().join("log").display().to_string();
        let at = tools.path().join("where").display().to_string();
        let script = |path: &Path, body: String| {
            std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        script(
            &tools.path().join("moon.in"),
            format!(
                "echo \"moon $*\" >> '{log}'\npwd -P >> '{at}'\n\
                 if [ \"$1\" = --version ]; then echo 'moon 2.6.1'; fi\n"
            ),
        );
        script(
            &bin.join("proto"),
            format!(
                "echo \"proto $*\" >> '{log}'\npwd -P >> '{at}'\n\
                 if [ \"$1\" = install ]; then cp '{tools}/moon.in' '{bin}/moon'; fi\n",
                tools = tools.path().display(),
                bin = bin.display(),
            ),
        );
        script(
            &tools.path().join("toolchain"),
            format!(
                "echo toolchain >> '{log}'\npwd -P >> '{at}'\n\
                 echo 'export PATH=\"{bin}:$PATH\"'\n",
                bin = bin.display(),
            ),
        );
        let repo = Repo::new(name);
        repo.write(
            ".config/sluice.toml",
            &format!(
                "[tests]\nrunner = \"moon\"\ntoolchain = [\"/bin/sh\", \"{}\"]\n\n\
                 [checkout]\nports = [\"web\"]\nports_from = \"44100\"\n\n\
                 [branches.main]\ntimeout = \"1m\"\n\n\
                 [preview]\ncommand = \"moon run app:preview\"\n",
                tools.path().join("toolchain").display()
            ),
        );
        repo.write(".prototools", "moon = \"2.6.1\"\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "pins and a policy"]);
        let engine_dir = Scratch::short();
        Setup {
            repo,
            tools,
            config: Scratch::new(&format!("{name}-config")),
            state: Scratch::new(&format!("{name}-state")),
            worktrees: Scratch::new(&format!("{name}-worktrees")),
            engine: support::containers_engine(engine_dir.path()),
            _engine_dir: engine_dir,
        }
    }

    /// A worktree on `unit/<name>` off `main`, made by git and never by
    /// sluice: its directory, canonical.
    fn worktree(&self, name: &str) -> PathBuf {
        let dir = self.worktrees.path().join(name);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            "main",
        ]);
        std::fs::canonicalize(dir).unwrap()
    }

    /// The binary run in `dir`, out of reach of the host's own sluice and
    /// of every credential.
    fn sluice(&self, dir: &Path, args: &[&str]) -> Output {
        support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
            .args(args)
            .current_dir(dir)
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env(
                sluice::profiles::checks::ENGINE_SOCKETS_VAR,
                self.engine.socket(),
            )
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Each call the toolchain and the stand-ins logged, in order.
    fn log(&self) -> Vec<String> {
        lines(&self.tools.path().join("log"))
    }

    /// The folder each call ran in, in order.
    fn ran_in(&self) -> Vec<String> {
        lines(&self.tools.path().join("where"))
    }

    /// What the host holds for `worktree`: the session runs and the ports
    /// in its state, and whether the repository keeps a place's record of
    /// the worktree.
    fn held(&self, worktree: &Path) -> (Vec<String>, Vec<String>, bool) {
        let names = |dir: &str| -> Vec<String> {
            match std::fs::read_dir(self.state.path().join("sluice").join(dir)) {
                Ok(d) => d
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.starts_with('.'))
                    .collect(),
                Err(_) => Vec::new(),
            }
        };
        let record = self.repo.dir().join(format!(
            ".git/sluice/.places/{}.json",
            sluice::runs::flat_name(&format!("session/{}", worktree.display()))
        ));
        (names("sessions"), names("ports"), record.exists())
    }
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(out)))
}

/// `EQUIPPED`, then `then`.
fn equipped_then(then: &[&str]) -> Vec<String> {
    EQUIPPED.iter().chain(then).map(|s| s.to_string()).collect()
}

/// 147, S72, 89; rule core.46: `sluice exec main -- moon run app:test`
/// in a worktree whose pins nothing has installed installs them, checks
/// moon's version and runs `moon setup` there before the command, which
/// then finds moon and passes.
#[test]
fn exec_equips_before_its_command() {
    let s = Setup::new("equip-tools-exec");
    let w = s.worktree("a");
    let out = s.sluice(&w, &["exec", "main", "--", "moon", "run", "app:test"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(s.log(), equipped_then(&["moon run app:test"]));
    let top = w.display().to_string();
    assert!(s.ran_in().iter().all(|d| *d == top), "{:?}", s.ran_in());
}

/// 147, S72, 98; rule core.46: a preview's checkout is equipped through
/// `moon setup` before its command, which then finds moon, and the
/// preview is up.
#[test]
fn a_preview_is_equipped_before_its_command() {
    let s = Setup::new("equip-tools-preview");
    let rev = s.repo.rev("main");
    let out = s.sluice(s.repo.dir(), &["preview", "main", &rev, "--wait", "--json"]);
    let doc = json(&out);
    assert_eq!(doc["state"], "up", "{doc}");
    assert_eq!(s.log(), equipped_then(&["moon run app:preview"]));
    let checkout = std::fs::canonicalize(doc["checkout"].as_str().unwrap()).unwrap();
    let checkout = checkout.display().to_string();
    assert!(
        s.ran_in().iter().all(|d| *d == checkout),
        "{:?}",
        s.ran_in()
    );
    let out = s.sluice(s.repo.dir(), &["release", doc["name"].as_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.engine.listings(), 1, "its release asked its own engine");
}

/// 147, S72, the command; rule core.46: `sluice equip main` in a
/// worktree sluice never ran in runs the same four steps in the same
/// order, each passing, and answers them in one document on stdout.
#[test]
fn equip_installs_the_pins_before_moon_runs() {
    let s = Setup::new("equip-tools-pins");
    let w = s.worktree("a");
    let out = s.sluice(&w, &["equip", "main", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let doc = json(&out);
    assert_eq!(doc["branch"], "main", "{doc}");
    assert_eq!(doc["revision"], s.repo.rev("main"), "{doc}");
    let steps: Vec<(&str, bool)> = doc["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["step"].as_str().unwrap(), s["passed"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        steps,
        [
            ("toolchain", true),
            ("proto install", true),
            ("moon --version", true),
            ("moon setup", true),
        ]
    );
    assert_eq!(s.log(), EQUIPPED);
}

/// 147, S72, 71; rules core.16, core.18, core.46: after `sluice equip`
/// the host holds no session run, no ports and no place's record for the
/// worktree, nothing of the equip's own is left among the runs, and
/// `sluice release` there finds nothing to release.
#[test]
fn equip_adopts_nothing() {
    let s = Setup::new("equip-tools-adopts");
    let w = s.worktree("a");
    let before = s.held(&w);
    assert_eq!(before, (vec![], vec![], false));
    let out = s.sluice(&w, &["equip", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(s.held(&w), before);
    let left: Vec<String> = std::fs::read_dir(s.repo.dir().join(".git/sluice/.runs"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    let out = s.sluice(&w, &["release"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(text(&out), "nothing to release\n");
}

/// 147, S72, the command: run in a folder below the worktree's top,
/// `sluice equip` equips the top, where the `.prototools` is, and says
/// so first.
#[test]
fn equip_runs_at_the_top_of_the_worktree() {
    let s = Setup::new("equip-tools-top");
    let w = s.worktree("a");
    let below = w.join("app/src");
    std::fs::create_dir_all(&below).unwrap();
    let out = s.sluice(&below, &["equip", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    let rev = s.repo.rev("main");
    assert_eq!(
        said.lines().next().unwrap(),
        format!("equipping {} for main at {}", w.display(), &rev[..7]),
        "{said}"
    );
    for step in ["toolchain", "proto install", "moon --version", "moon setup"] {
        assert!(said.contains(&format!("\n{step}: passed\n")), "{said}");
    }
    assert_eq!(s.log(), EQUIPPED);
    let top = w.display().to_string();
    assert!(s.ran_in().iter().all(|d| *d == top), "{:?}", s.ran_in());
}

/// 147, S72, the command: a revision that is no commit here is refused,
/// exiting 2, and nothing is run.
#[test]
fn equip_refuses_a_revision_it_cannot_read() {
    let s = Setup::new("equip-tools-no-rev");
    let w = s.worktree("a");
    let out = s.sluice(&w, &["equip", "main", "--at", "nosuch"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("refused: nosuch names no revision here"),
        "{}",
        text(&out)
    );
    assert!(s.log().is_empty(), "{:?}", s.log());
}

/// 147, S72, the command: run in a repository's bare git directory,
/// `sluice equip` refuses the folder as in no worktree, with no steps,
/// exiting 2, and nothing is run.
#[test]
fn equip_refuses_a_folder_in_no_worktree() {
    let s = Setup::new("equip-tools-bare");
    let bare = s.repo.bare_origin();
    let out = s.sluice(bare.path(), &["equip", "main", "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "refused", "{doc}");
    assert_eq!(
        doc["error"]["message"],
        format!("{} is in no worktree", bare.path().display()),
        "{doc}"
    );
    assert!(doc.get("steps").is_none(), "{doc}");
    assert!(s.log().is_empty(), "{:?}", s.log());
}

/// resident-workers, "sluice equip with no tools of its own" (the change
/// `sluice-host-runs-a-sessions-checks`, design D1): with `sluice host`
/// started with the toolchain and the task runner on its `PATH`, `sluice
/// equip main` in a worktree with neither on its own prints each step of
/// the equip, as it printed them before, exits 0, and runs no git but the
/// branch protection's upkeep.
#[test]
fn sluice_equip_with_no_tools_of_its_own() {
    let s = Setup::new("equip-tools-none-here");
    let w = s.worktree("a");
    let command = |dir: &Path| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
        support::isolated(&mut c)
            .current_dir(dir)
            .env("XDG_CONFIG_HOME", s.config.path())
            .env("XDG_STATE_HOME", s.state.path())
            .env(
                sluice::profiles::checks::ENGINE_SOCKETS_VAR,
                s.engine.socket(),
            )
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        c
    };
    let held = sluice::host_process::client::Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(&command(&w)),
    )
    .unwrap()
    .subscribe(Vec::new())
    .expect("the host process starts");
    let git = support::CountingGit::new();
    let mut c = command(&w);
    c.args(["equip", "main"]).env("PATH", "/usr/bin:/bin");
    git.on(&mut c);
    let client = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = client.id();
    let out = client.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    let rev = s.repo.rev("main");
    assert_eq!(
        said.lines().next().unwrap(),
        format!("equipping {} for main at {}", w.display(), &rev[..7]),
        "{said}"
    );
    for step in ["toolchain", "proto install", "moon --version", "moon setup"] {
        assert!(said.contains(&format!("\n{step}: passed\n")), "{said}");
    }
    assert_eq!(s.log(), EQUIPPED);
    for (_, call) in git.parented().into_iter().filter(|(p, _)| *p == pid) {
        assert!(
            [
                "rev-parse --path-format=absolute --git-path hooks",
                "rev-parse --path-format=absolute --git-common-dir",
                "worktree list --porcelain",
            ]
            .iter()
            .any(|u| call.ends_with(u)),
            "the client ran git {call}"
        );
    }
    drop(held);
    support::guard::stop_host_processes(s.state.path());
}
