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
use crate::overview::{no_sluice, untimed};
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
        if let Err(e) = self.process.pass_of(self.here, repository, clock) {
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
    // A pass is asked of the folder: `sluice host` resolves it.
    let asked = sub.folders[0].folder.clone();
    loop {
        sub.ask_pass(Some(&asked), clock);
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

/// `sluice run`'s part in following `sluice host` (design D8): a
/// subscription of its folder as `run`, each pass's report as it comes,
/// and a pass asked at a tick that brought none, so each of its rounds is
/// a pass; where `sluice host` passes only when asked, a pass asked at
/// each of its ticks, whose report is the pass's (D14).
pub struct Runner<'a> {
    reached: Reached<'a>,
    folder: PathBuf,
    tick: Duration,
    waiting: std::collections::VecDeque<crate::host_process::pass::Report>,
    /// Whether the last pass found nothing in play, so no pass comes at a
    /// tick of the host process's own.
    idle: bool,
}

impl<'a> Runner<'a> {
    pub fn new(
        process: &'a dyn HostProcess,
        here: Option<&'a Here<'a, 'a>>,
        folder: PathBuf,
        tick: Duration,
    ) -> Runner<'a> {
        let folders = vec![Added {
            folder: folder.clone(),
            kind: FollowKind::Run,
        }];
        Runner {
            reached: Reached::new(process, here, folders),
            folder,
            tick,
            waiting: Default::default(),
            idle: true,
        }
    }

    /// The next pass's report, waiting up to a tick for one; `None` where
    /// none came, or the host process could not be reached.
    pub fn next(&mut self, clock: &dyn Clock) -> Option<crate::host_process::pass::Report> {
        self.reached.open()?;
        if self.reached.asked_only() {
            // The passes the subscription reports are those it asked.
            self.reached.lines(Duration::from_millis(1));
            return match self
                .reached
                .process
                .pass_of(self.reached.here, &self.folder, clock)
            {
                Ok(r) => Some(r),
                Err(e) => {
                    telemetry::warn(&format!("sluice: warning: {e:#}"));
                    None
                }
            };
        }
        if self.waiting.is_empty() {
            for line in self.reached.lines(self.tick) {
                if let SubscriptionLine::Pass(p) = line {
                    self.waiting.push_back(p.report);
                }
            }
        }
        // A repository with nothing in play has no pass at a tick of its
        // own: a tick with no pass asks one, so each round is a pass. The
        // pass comes as each pass does, on the subscription.
        if self.waiting.is_empty() && self.idle && self.reached.open.is_some() {
            if let Err(e) = self
                .reached
                .process
                .pass_of(self.reached.here, &self.folder, clock)
            {
                telemetry::warn(&format!("sluice: warning: {e:#}"));
            }
            for line in self.reached.lines(self.tick) {
                if let SubscriptionLine::Pass(p) = line {
                    self.waiting.push_back(p.report);
                }
            }
        }
        let report = self.waiting.pop_front();
        if let Some(r) = &report {
            self.idle = !r.in_play;
        }
        report
    }

    /// Whether the host process passes only when asked.
    pub fn asked_only(&self) -> bool {
        self.reached.asked_only()
    }
}

/// A wait's part in following `sluice host` (design D8): a subscription
/// of its folder as `wait`, from whose documents it reads its submission,
/// its combination and its branch's hold, and whose pass reports say what
/// each pass released and where a round failed; where `sluice host`
/// passes only when asked, a pass asked at each of its ticks (D14).
pub struct Waiter<'a> {
    reached: Reached<'a>,
    folder: PathBuf,
    document: Option<serde_json::Value>,
}

impl<'a> Waiter<'a> {
    pub fn new(
        process: &'a dyn HostProcess,
        here: Option<&'a Here<'a, 'a>>,
        folder: PathBuf,
    ) -> Waiter<'a> {
        let folders = vec![Added {
            folder: folder.clone(),
            kind: FollowKind::Wait,
        }];
        Waiter {
            reached: Reached::new(process, here, folders),
            folder,
            document: None,
        }
    }

    /// The last document that `sluice host` sent; `None` before the first.
    pub fn document(&self) -> Option<&serde_json::Value> {
        self.document.as_ref()
    }

    /// The document `sluice host` sends as the subscription opens, where it
    /// holds one: no pass is asked.
    pub fn look(&mut self) {
        if self.reached.open().is_some() {
            self.take_lines(Duration::from_millis(1));
        }
    }

    /// One tick: a pass asked first where passes are taken only when
    /// asked; the newest document, and the reports of the passes since the
    /// last tick.
    pub fn tick(&mut self, clock: &dyn Clock) -> Vec<crate::host_process::pass::Report> {
        self.take(clock, Duration::from_millis(1))
    }

    /// Its first tick: as [`Waiter::tick`], waiting up to `wait` for the
    /// first document where none came yet.
    pub fn first(
        &mut self,
        clock: &dyn Clock,
        wait: Duration,
    ) -> Vec<crate::host_process::pass::Report> {
        let until = std::time::Instant::now() + wait;
        let mut reports = self.take(clock, LINE_WAIT);
        while self.document.is_none() && std::time::Instant::now() < until {
            if INTERRUPTED.load(Ordering::SeqCst) || self.reached.open.is_none() {
                break;
            }
            reports.extend(self.take_lines(LINE_WAIT));
        }
        reports
    }

    fn take(
        &mut self,
        clock: &dyn Clock,
        wait: Duration,
    ) -> Vec<crate::host_process::pass::Report> {
        let mut reports = Vec::new();
        if self.reached.open().is_none() {
            return reports;
        }
        if self.reached.asked_only() {
            match self
                .reached
                .process
                .pass_of(self.reached.here, &self.folder, clock)
            {
                Ok(r) => reports.push(r),
                Err(e) => telemetry::warn(&format!("sluice: warning: {e:#}")),
            }
            // The passes the subscription reports are those it asked.
            self.take_lines(wait);
            return reports;
        }
        reports.extend(self.take_lines(wait));
        reports
    }

    fn take_lines(&mut self, wait: Duration) -> Vec<crate::host_process::pass::Report> {
        let mut reports = Vec::new();
        for line in self.reached.lines(wait) {
            match line {
                SubscriptionLine::Document(d) => self.document = Some(d.overview),
                SubscriptionLine::Pass(p) => reports.push(p.report),
                SubscriptionLine::Error(_) => {}
            }
        }
        reports
    }
}
