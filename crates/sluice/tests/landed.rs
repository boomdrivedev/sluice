//! A landing stands whatever fails after it (122), through the binary on
//! a scratch repository under local refs: the release of the
//! combination's checkout fails once the landing changed the script it
//! runs, and the waiting submit still reports the landing and exits 0,
//! the checkout held and the round going on (123).
//! Needs git, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::process::{Command, Stdio};

use support::{Repo, Scratch};

const POLICY: &str =
    "[tests]\nrunner = \"command\"\ncommand = \"true\"\nrelease = \"sh release.sh\"\n\n\
[branches.\"bolt/*\"]\ntimeout = \"1m\"\n";

/// 122, 123: the branch moves to the submission's landing, then the
/// release command, run in the checkout of the combination that landed,
/// fails: the waiting submit says the failed try and the landing as for
/// any other, and exits 0; the round went on, so nothing failed after
/// the landing.
#[test]
fn a_release_failing_after_the_landing_leaves_the_submit_landed() {
    let repo = Repo::new("landed");
    repo.write(".config/sluice.toml", POLICY);
    repo.write("release.sh", "exit 0\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "sluice's policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let sessions = Scratch::new("landed-sessions");
    let worktree = sessions.path().join("a");
    let at = worktree.to_str().unwrap();
    repo.git(&["worktree", "add", "-q", "-b", "unit/a", at, "bolt/x"]);
    // The landed script takes other arguments than the release gives it.
    std::fs::write(
        worktree.join("release.sh"),
        "echo release.sh takes only up\nexit 1\n",
    )
    .unwrap();
    repo.git(&["-C", at, "add", "-A"]);
    repo.git(&["-C", at, "commit", "-q", "-m", "release.sh takes only up"]);

    let config = Scratch::new("landed-config");
    let state = Scratch::new("landed-state");
    let out = support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
        .args(["submit", "bolt/x"])
        .current_dir(&worktree)
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .env_remove("SLUICE_AGENT")
        .env_remove("SLUICE_OTEL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");
    let tip = repo.rev("bolt/x");
    assert!(
        stdout.ends_with(&format!("passed · landed as {}\n", &tip[..7])),
        "{stdout}{stderr}"
    );
    assert!(
        !stderr.contains("sluice: after the landing: "),
        "{stdout}{stderr}"
    );
    let tried = stdout
        .lines()
        .find(|l| l.starts_with("could not release sluice/try/bolt/x/"))
        .unwrap_or_else(|| panic!("{stdout}{stderr}"));
    assert!(
        tried.contains("(try 1 of 4): the release command of ")
            && tried.contains("release.sh takes only up"),
        "{stdout}{stderr}"
    );
}

/// 122, 123: a waiting submit follows `sluice host` before it submits, so
/// the pass that its submission starts reports to it however soon that
/// pass ends. Here the submit is held from the moment `sluice host` reads
/// its submit until that pass has tried the release, as a busy host may
/// hold a command between its submit's answer and its first read: once
/// it goes on, it still says the release's first try and the landing.
#[test]
fn a_waiting_submit_held_past_its_pass_says_what_the_pass_released() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let repo = Repo::new("landed-held");
    repo.write(".config/sluice.toml", POLICY);
    repo.write("release.sh", "exit 0\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "sluice's policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let sessions = Scratch::new("landed-held-sessions");
    let marks = Scratch::new("landed-held-marks");
    let worktree = sessions.path().join("a");
    let at = worktree.to_str().unwrap();
    repo.git(&["worktree", "add", "-q", "-b", "unit/a", at, "bolt/x"]);
    // The landed script marks each try of it, and fails.
    std::fs::write(
        worktree.join("release.sh"),
        format!(
            "echo tried >> '{}'\necho release.sh takes only up\nexit 1\n",
            marks.path().join("released").display()
        ),
    )
    .unwrap();
    repo.git(&["-C", at, "add", "-A"]);
    repo.git(&["-C", at, "commit", "-q", "-m", "release.sh takes only up"]);

    let config = Scratch::new("landed-held-config");
    let state = Scratch::new("landed-held-state");
    let child = support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
        .args(["submit", "bolt/x"])
        .current_dir(&worktree)
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .env_remove("SLUICE_AGENT")
        .env_remove("SLUICE_OTEL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    /// The submit, let go on again and killed should the test end first.
    struct Held(Option<std::process::Child>);
    impl Drop for Held {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGCONT);
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut held = Held(Some(child));
    let pid = Pid::from_raw(held.0.as_ref().unwrap().id() as i32);
    let log = state.path().join("sluice/host/log");
    let read_submit = |log: &std::path::Path| {
        std::fs::read_to_string(log).is_ok_and(|l| l.contains(r#"change {"ask":"submit""#))
    };
    let until = Instant::now() + Duration::from_secs(60);
    while !read_submit(&log) {
        assert!(Instant::now() < until, "sluice host never read the submit");
        std::thread::sleep(Duration::from_millis(1));
    }
    kill(pid, Signal::SIGSTOP).unwrap();
    let released = marks.path().join("released");
    let until = Instant::now() + Duration::from_secs(60);
    while !released.exists() {
        assert!(Instant::now() < until, "the release was never tried");
        std::thread::sleep(Duration::from_millis(50));
    }
    kill(pid, Signal::SIGCONT).unwrap();
    let out = held.0.take().unwrap().wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");
    let tip = repo.rev("bolt/x");
    assert!(
        stdout.ends_with(&format!("passed · landed as {}\n", &tip[..7])),
        "{stdout}{stderr}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("could not release sluice/try/bolt/x/")
                && l.contains("(try 1 of 4): the release command of ")),
        "{stdout}{stderr}\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
}
