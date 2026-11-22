//! The clients of a subscription to `sluice host`: a watch of one folder,
//! a watch of the folders on its input, and `sluice run` (the change
//! `sluice-host-answers-every-read`, design D6, D7, D8;
//! `reads-go-through-the-resident@69134e5825d7`).
//!
//! A client reads what `sluice host` sends and prints it. It reads no
//! record and builds no runtime: on the socket it reads lines on std's
//! `UnixStream`. Where `sluice host` passes only when asked, the client
//! asks a pass at each of its ticks (D14). A client whose `sluice host`
//! ended reaches it again at its next tick, starting one where none
//! answers, and keeps the last document that it got meanwhile.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use sluice_core::Clock;

use crate::host_process::protocol::{Added, FollowKind, SubscriptionLine};
use crate::host_process::{Here, HostProcess, Subscription};
use crate::reader::{no_sluice, untimed};
use crate::{telemetry, Pause, INTERRUPTED, STOPPED};

/// How long a client waits for a line before it looks at its own state
/// again: a watch's tick.
const LINE_WAIT: Duration = Duration::from_millis(100);

/// A subscription that is reached again at a tick after it ended.
struct Reached<'a> {
    process: &'a dyn HostProcess,
    here: Option<&'a Here<'a, 'a>>,
    folders: Vec<Added>,
    open: Option<Box<dyn Subscription + 'a>>,
}

impl<'a> Reached<'a> {
    fn new(
        process: &'a dyn HostProcess,
        here: Option<&'a Here<'a, 'a>>,
        folders: Vec<Added>,
    ) -> Reached<'a> {
        Reached {
            process,
            here,
            folders,
            open: None,
        }
    }

    /// The subscription, opened again where it ended; `None` where the
    /// host process could not be reached, said on stderr.
    fn open(&mut self) -> Option<&mut Box<dyn Subscription + 'a>> {
        if self.open.is_none() {
            match self.process.subscribe(self.here, self.folders.clone()) {
                Ok(s) => self.open = Some(s),
                Err(e) => telemetry::warn(&format!(
                    "sluice: warning: sluice's host process could not be reached; tried again at the next tick: {e:#}"
                )),
            }
        }
        self.open.as_mut()
    }

    /// Whether the host process passes only when asked, as its subscription
    /// said, else as this command's own setting says.
    fn asked_only(&self) -> bool {
        self.open
            .as_ref()
            .map_or_else(crate::host_process::asked_only, |s| s.asked_only())
    }

    /// Each line that came, waiting `wait` for the first; the subscription
    /// let go where the host process ended under it.
    fn lines(&mut self, wait: Duration) -> Vec<SubscriptionLine> {
        let mut lines = Vec::new();
        let Some(open) = self.open() else {
            return lines;
        };
        let mut wait = wait;
        loop {
            match open.next(wait) {
                Ok(Some(line)) => lines.push(line),
                Ok(None) => break,
                Err(_) => {
                    self.open = None;
                    break;
                }
            }
            wait = Duration::from_millis(1);
        }
        lines
    }

    /// Ask a pass of `repository` where the host process passes only when
    /// asked (D14).
    fn ask_pass(&mut self, repository: Option<&Path>, clock: &dyn Clock) {
        let (true, Some(repository)) = (self.asked_only(), repository) else {
            return;
        };
        if let Err(e) = self
            .process
            .pass_of(self.here, repository, Some(clock.now()))
        {
            telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
    }
}

/// `sluice overview --json --watch` of `folder` (design D7): the overview
/// at the start and at each change in a part other than its time, one
/// line each, as `sluice host` sends it. It ends where the repository has
/// no sluice, after that overview, with 0; where the repository is gone,
/// with that failure; and when stopped, with 130.
pub fn watch<'a>(
    process: &'a dyn HostProcess,
    here: Option<&'a Here<'a, 'a>>,
    folder: PathBuf,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    let mut sub = Reached::new(
        process,
        here,
        vec![Added {
            folder,
            kind: FollowKind::Watch,
        }],
    );
    let mut last: Option<serde_json::Value> = None;
    let mut repository: Option<PathBuf> = None;
    loop {
        sub.ask_pass(repository.as_deref(), clock);
        for line in sub.lines(LINE_WAIT) {
            let SubscriptionLine::Document(d) = line else {
                continue;
            };
            let doc = d.overview;
            // A repository gone ends the watch as a failing overview ends,
            // with the words `sluice host` said.
            if doc["gone"] == true {
                let words = doc["error"]["message"].as_str().unwrap_or("gone");
                anyhow::bail!("{words}");
            }
            if let Some(r) = doc["repository"].as_str() {
                repository = Some(PathBuf::from(r));
            }
            let now = untimed(&doc);
            if last.as_ref() != Some(&now) {
                writeln!(out, "{}", serde_json::to_string(&doc)?)?;
                out.flush()?;
                last = Some(now);
            }
            if no_sluice(&doc) {
                return Ok(0);
            }
        }
        if INTERRUPTED.load(Ordering::SeqCst) || !pause.pause() {
            return Ok(STOPPED);
        }
    }
}

/// One line of `sluice overview --json --watch --stdin`'s input: `add
/// <folder>` or `remove <folder>`, a relative folder read from `cwd`;
/// `None` for a line that is neither.
fn input_line(line: &str, cwd: &Path) -> Option<crate::host_process::protocol::SubscriptionAsk> {
    use crate::host_process::protocol::SubscriptionAsk;
    let (word, folder) = line.trim().split_once(' ')?;
    let folder = cwd.join(folder.trim());
    match word {
        "add" => Some(SubscriptionAsk::Add {
            add: folder,
            kind: None,
        }),
        "remove" => Some(SubscriptionAsk::Remove { remove: folder }),
        _ => None,
    }
}

/// `sluice overview --json --watch --stdin` (design D7): one subscription,
/// whose folders the lines of standard input add and remove, each
/// document printed as one line with `folder` in it. The main thread reads
/// the socket; the input thread reads standard input and writes each line
/// on a second handle of the stream. At the end of its input, where no
/// folder stays, the input thread marks that the input ended and shuts the
/// stream down, and the command exits 0; where folders stay, the watch
/// goes on. Where the socket's read ends with no such mark, `sluice host`
/// ended, and the command fails (exit 3).
pub fn watch_input(cwd: &Path, out: &mut dyn Write) -> Result<i32> {
    use std::io::BufRead;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    let client = crate::host_process::client::Client::host()?;
    let mut sub = client.subscribe(Vec::new())?;
    let asked_only = sub.named.asked_only;
    let mut writer = sub.stream()?;
    let ended = Arc::new(AtomicBool::new(false));
    let input_ended = ended.clone();
    let cwd = cwd.to_path_buf();
    // The input thread ends with the process: a signal ends both.
    std::thread::spawn(move || {
        let mut folders = std::collections::BTreeSet::new();
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Some(ask) = input_line(&line, &cwd) else {
                continue;
            };
            match &ask {
                crate::host_process::protocol::SubscriptionAsk::Add { add, .. } => {
                    folders.insert(add.clone());
                }
                crate::host_process::protocol::SubscriptionAsk::Remove { remove } => {
                    folders.remove(remove);
                }
            }
            if crate::host_process::protocol::send(&mut writer, &ask).is_err() {
                return;
            }
        }
        if folders.is_empty() {
            input_ended.store(true, Ordering::SeqCst);
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
    });
    let mut repositories = std::collections::BTreeSet::new();
    loop {
        match sub.line(Some(Duration::from_secs(1))) {
            Ok(Some(SubscriptionLine::Document(d))) => {
                if let Some(r) = d.overview["repository"].as_str() {
                    repositories.insert(PathBuf::from(r));
                }
                let mut line = serde_json::json!({ "folder": d.folder });
                if let (Some(line), Some(doc)) = (line.as_object_mut(), d.overview.as_object()) {
                    line.extend(doc.clone());
                }
                writeln!(out, "{}", serde_json::to_string(&line)?)?;
                out.flush()?;
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                if INTERRUPTED.load(Ordering::SeqCst) {
                    return Ok(STOPPED);
                }
                // D14: where passes are taken only when asked, a pass of
                // each repository at each tick.
                if asked_only {
                    for r in &repositories {
                        let _ = client.pass(r, None);
                    }
                }
            }
            Err(e) => {
                if ended.load(Ordering::SeqCst) {
                    return Ok(0);
                }
                anyhow::bail!("sluice's host process did not answer: {e:#}");
            }
        }
    }
}
