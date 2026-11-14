//! A kept `sluice run`, a follower of sluice's host process, that takes up
//! a change of the host's settings that moves its own telemetry,
//! `[telemetry]`, by starting itself afresh in its own process (135 as
//! amended 2026-11-09; record 0107 as amended; rule core.54; design D2 to
//! D5 of `a-kept-run-hands-over-what-it-cannot-take-up`, and D11 of
//! `one-sluice-process-runs-each-hosts-queues`). It holds nothing else its
//! settings decide: the host process holds the queues, their driver's
//! lock and the runs' supervisors.
//!
//! The run replaces its own program image, so the process keeps its id,
//! its parent, its terminal, its output, its folder and its session, and
//! whatever runs it goes on following it. The image started afresh is the
//! run as it started: the executable it started as, resolved through every
//! link, the command line it was given, and the environment it started
//! with, before telemetry took the caller's OTLP variables out of it
//! (151; rule core.53). What the run carries, it writes as one JSON
//! document into [`VAR`]:
//!
//! - the settings file's text the run read and bound, and the tables that
//!   changed;
//! - the passes it followed toward `--rounds`;
//! - whether it said that nothing needs `sluice run`.
//!
//! The form needs no version: one executable writes it and reads it. The
//! image started afresh reads it first, before any thread or child
//! starts, and takes it out of its environment, so no child inherits it.

use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// The variable the document is handed over in.
pub const VAR: &str = "SLUICE_HANDED_OVER";

/// What a `sluice run` hands to itself started afresh (design D4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOver {
    /// The settings file's text the run read and bound.
    pub settings: String,
    /// The tables that changed, as `Instance::changed_tables` names them.
    pub tables: Vec<String>,
    /// The passes followed toward `--rounds`.
    pub passes: usize,
    /// Whether the run said that nothing needs `sluice run`.
    pub told: bool,
}

impl HandOver {
    /// The document handed over, where this process is a `sluice run`
    /// started afresh. [`VAR`] is taken out of the environment, so no child
    /// inherits it. Called first in `main`, before any thread or child
    /// starts.
    pub fn take() -> Result<Option<HandOver>> {
        let Some(text) = std::env::var_os(VAR) else {
            return Ok(None);
        };
        std::env::remove_var(VAR);
        let text = text
            .into_string()
            .map_err(|_| anyhow!("{VAR} is not UTF-8"))?;
        let handed: HandOver =
            serde_json::from_str(&text).with_context(|| format!("reading {VAR}"))?;
        Ok(Some(handed))
    }

    /// The command that starts the run afresh: `program`, with `args`,
    /// its environment cleared and set to `env`, but for [`VAR`], which
    /// holds this document (design D3, D4).
    pub fn command(
        &self,
        program: &Path,
        args: &[OsString],
        env: &[(OsString, OsString)],
    ) -> Result<Command> {
        let doc = serde_json::to_string(self).context("writing the hand-over")?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .envs(env.iter().filter(|(name, _)| name != VAR).cloned())
            .env(VAR, doc);
        Ok(cmd)
    }

    /// Replace this process's image with the run started afresh (design
    /// D2, D5). It returns only where the start failed, with the process as
    /// it was (rule core.54), and the error names `program`.
    pub fn start(
        self,
        program: &Path,
        args: &[OsString],
        env: &[(OsString, OsString)],
    ) -> anyhow::Error {
        let failed = match self.command(program, args, env) {
            Ok(cmd) => replace(&cmd),
            Err(e) => e,
        };
        failed.context(format!("starting {} afresh", program.display()))
    }
}

/// Replace this process's image with `cmd`'s by `execve`, `cmd` being one
/// [`HandOver::command`] built, its environment cleared and set whole
/// (design D2, D3). It returns only where the start failed, and leaves
/// the process as it was. std's `CommandExt::exec` does not: while it
/// tries the start it sets the process's own environment to the
/// command's, so a thread reading a variable meanwhile reads the
/// command's, or memory freed once the start failed, and it sets
/// `SIGPIPE` back to its default, which a run whose start failed would
/// keep.
fn replace(cmd: &Command) -> anyhow::Error {
    let c = |s: &OsStr| {
        CString::new(s.as_bytes()).map_err(|_| anyhow!("{} holds a NUL", s.to_string_lossy()))
    };
    let parts = || -> Result<(CString, Vec<CString>, Vec<CString>)> {
        let program = c(cmd.get_program())?;
        let mut argv = vec![program.clone()];
        for arg in cmd.get_args() {
            argv.push(c(arg)?);
        }
        let mut envp = Vec::new();
        for (name, value) in cmd.get_envs() {
            if let Some(value) = value {
                let mut pair = name.to_owned();
                pair.push("=");
                pair.push(value);
                envp.push(c(&pair)?);
            }
        }
        Ok((program, argv, envp))
    };
    match parts() {
        Ok((program, argv, envp)) => match nix::unistd::execve(&program, &argv, &envp) {
            Ok(never) => match never {},
            Err(errno) => std::io::Error::from(errno).into(),
        },
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    /// A document with every field set.
    fn every() -> HandOver {
        HandOver {
            settings: "[telemetry]\nsend = true\ncollector = \"http://localhost:4318\"\n".into(),
            tables: vec!["[telemetry]".into(), "[[caches]]".into()],
            passes: 4,
            told: true,
        }
    }

    /// The settings-take-up spec's "The hand-over's command": the run
    /// started afresh is the executable, the command line and the
    /// environment the run started with, its OTLP variable among it, and
    /// the document, which reads back as written (151; core.53; design
    /// D3, D4).
    #[test]
    fn the_run_started_afresh_is_the_one_that_started() {
        let handed = every();
        let args: Vec<OsString> = ["run", "--rounds", "12"].map(OsString::from).into();
        let env: Vec<(OsString, OsString)> = vec![
            ("HOME".into(), "/home/someone".into()),
            (
                "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
                "http://localhost:9999".into(),
            ),
        ];
        let cmd = handed.command(Path::new("/x/sluice"), &args, &env).unwrap();
        assert_eq!(cmd.get_program(), "/x/sluice");
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            ["run", "--rounds", "12"]
        );
        let mut set: BTreeMap<OsString, Option<OsString>> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
            .collect();
        let doc = set
            .remove(&OsString::from(VAR))
            .flatten()
            .expect("the document");
        let expected: BTreeMap<OsString, Option<OsString>> =
            env.into_iter().map(|(k, v)| (k, Some(v))).collect();
        assert_eq!(set, expected);
        assert!(
            format!("{cmd:?}").starts_with("env -i "),
            "every other variable is cleared: {cmd:?}"
        );
        let read: HandOver = serde_json::from_str(doc.to_str().unwrap()).unwrap();
        assert_eq!(read, handed);
    }

    /// A start that fails leaves the process as it was: no thread reads
    /// the environment handed over while the start is tried, which reads
    /// no variable of the process's own meanwhile, `TMPDIR` among them,
    /// and `SIGPIPE` keeps its disposition (core.54; design D5).
    #[test]
    fn a_hand_over_that_cannot_start_leaves_the_process_as_it_was() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;
        let sigpipe = || {
            let mut old = std::mem::MaybeUninit::<nix::libc::sigaction>::zeroed();
            // SAFETY: a null action only reads SIGPIPE's disposition into
            // `old`, which the call fills.
            unsafe {
                assert_eq!(
                    nix::libc::sigaction(nix::libc::SIGPIPE, std::ptr::null(), old.as_mut_ptr()),
                    0
                );
                old.assume_init().sa_sigaction
            }
        };
        let before = sigpipe();
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(AtomicUsize::new(0));
        let reader = {
            let (stop, seen) = (Arc::clone(&stop), Arc::clone(&seen));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if std::env::var_os(VAR).is_some() {
                        seen.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        };
        let program = std::env::temp_dir()
            .join(format!("sluice-hand-over-none-{}", std::process::id()))
            .join("sluice");
        for _ in 0..50 {
            let e = every().start(&program, &[], &[]);
            assert!(
                format!("{e:#}").contains("No such file or directory"),
                "{e:#}"
            );
        }
        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
        assert_eq!(
            seen.load(Ordering::Relaxed),
            0,
            "a thread read the environment handed over"
        );
        assert_eq!(sigpipe(), before, "SIGPIPE keeps its disposition");
    }

    /// The settings-take-up spec's "An executable that cannot start": the
    /// start answers the system's words naming the executable (core.54;
    /// design D5).
    #[test]
    fn a_hand_over_that_cannot_start_says_why() {
        let dir = std::env::temp_dir().join(format!("sluice-hand-over-{}", std::process::id()));
        let program = dir.join("no-such-sluice");
        let e = every().start(&program, &[], &[]);
        let words = format!("{e:#}");
        assert!(words.contains(&program.display().to_string()), "{words}");
        assert!(words.contains("No such file or directory"), "{words}");
    }
}
