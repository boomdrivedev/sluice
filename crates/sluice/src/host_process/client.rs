//! How a command reaches sluice's host process (135; design D2, D8): it
//! connects to the socket and says `hello` with its version. Where nothing
//! answers, it takes `host/start.lock`, tries once more, and starts the
//! process detached: in a session of its own, working in `/`, reading
//! nothing, its output in `host/log` and the previous process's kept as
//! `log.1`, with the host's own environment (D3). It waits up to ten
//! seconds for it to answer, and otherwise fails naming the log. Ten
//! commands at once take the start lock in turn: the first starts the
//! process, and the others find it answering. A command newer than the
//! process that answers has it hand over once; an older one uses the
//! process as it is. The process that hands over starts afresh in place as
//! the command's own executable, and the command uses the image started
//! afresh. A host process of an older sluice can end instead, and the
//! command then starts its own (design D3, D4 of `sluice-runs-as-a-daemon`;
//! D3 of `sluice-host-owns-its-workers`).

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, ErrorKind};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use super::pass::Report;
use super::protocol::{receive, send, Answer, Hello, Named, Output, Request};
use super::version::older;
use super::{environment, Paths};

/// How long a command waits for a process it started to answer (design
/// D2).
pub const START_WAIT: Duration = Duration::from_secs(10);

/// How long a newer command waits for an older host process it asked to
/// hand over to end: the time that process gives its tasks to finish the
/// rounds they are in, and a start's wait after (design D8).
pub const HAND_OVER_WAIT: Duration =
    Duration::from_secs(super::tasks::FINISH_FOR.as_secs() + START_WAIT.as_secs());

/// A command's way to the host process of one state folder.
#[derive(Clone, Debug)]
pub struct Client {
    /// The sluice executable a process is started from.
    program: PathBuf,
    paths: Paths,
    version: String,
    /// The environment of the command, which a process it starts keeps
    /// the host's own of (D3).
    env: Vec<(OsString, OsString)>,
}

/// A connection to the host process, past its `hello`.
pub struct Connection {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    /// The process that answered.
    pub named: Named,
}

/// Whether the process `pid` still runs: one of the user's own, a signal
/// it could be sent, or one it may not be sent.
fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    !matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

impl Client {
    /// `program`'s way to the host process a command whose environment is
    /// `env` reaches: that environment's state folder's.
    pub fn new(program: &Path, env: Vec<(OsString, OsString)>) -> Result<Client> {
        let var = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        Ok(Client {
            program: program.to_path_buf(),
            paths: Paths::from_env(var)?,
            version: crate::VERSION.to_string(),
            env,
        })
    }

    /// This sluice's way to its user's host process where one has run,
    /// making nothing: `None` where none has (record 0120 as amended
    /// 2026-11-17).
    pub fn found() -> Result<Option<Client>> {
        let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let var = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let Some(paths) = Paths::found_from_env(var)? else {
            return Ok(None);
        };
        Ok(Some(Client {
            program: std::env::current_exe()?,
            paths,
            version: crate::VERSION.to_string(),
            env,
        }))
    }

    /// This sluice's way to its user's host process.
    pub fn host() -> Result<Client> {
        Client::new(&std::env::current_exe()?, std::env::vars_os().collect())
    }

    /// The same client, saying it is sluice `version`.
    pub fn with_version(mut self, version: &str) -> Client {
        self.version = version.to_string();
        self
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The process that answers now, its connection past `hello`; `None`
    /// where none answers. It starts nothing.
    pub fn answering(&self) -> Result<Option<Connection>> {
        let stream = match UnixStream::connect(&self.paths.socket) {
            Ok(s) => s,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(None)
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("connecting to {}", self.paths.socket.display()))
            }
        };
        stream.set_read_timeout(Some(START_WAIT))?;
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        // A process ending drops what it no longer answers.
        if send(
            &mut writer,
            &Hello {
                hello: self.version.clone(),
            },
        )
        .is_err()
        {
            return Ok(None);
        }
        let Ok(named) = receive::<Named>(&mut reader) else {
            return Ok(None);
        };
        reader.get_ref().set_read_timeout(None)?;
        Ok(Some(Connection {
            reader,
            writer,
            named,
        }))
    }

    /// The host process, started where none answers, and an older one
    /// handed over once for this newer command's own (D8): started afresh
    /// in place as this command's executable, whose image started afresh is
    /// this command's own start (design D4 of `sluice-runs-as-a-daemon`; D3
    /// of `sluice-host-owns-its-workers`).
    pub fn reach(&self) -> Result<Connection> {
        if let Some(c) = self.answering()? {
            if !older(&c.named.version, &self.version) {
                return Ok(c);
            }
            return self.afresh(c);
        }
        self.start()
    }

    /// This command's executable, resolved through every link, which an
    /// older daemon starts afresh as (design D3 of `sluice-runs-as-a-daemon`).
    fn program(&self) -> Option<PathBuf> {
        std::fs::canonicalize(&self.program).ok()
    }

    /// D4 of `sluice-runs-as-a-daemon` and D3 of
    /// `sluice-host-owns-its-workers`: have the older host process `c` start
    /// afresh in place as this command's executable, and wait, as long as
    /// a hand-over is waited for ([`HAND_OVER_WAIT`]), for a host process
    /// that is not the image it asked, another pid or the same pid with
    /// another `since`, which it uses as a start of its own, whatever its
    /// version. A host process of an older sluice that ends instead lets
    /// its pid go, and the command then starts its own. Where the image it
    /// asked still answers, or its pid still runs, at the end of the wait,
    /// it fails naming the log.
    fn afresh(&self, mut c: Connection) -> Result<Connection> {
        let asked = c.named.clone();
        send(
            &mut c.writer,
            &Request::HandOver {
                program: self.program(),
                ends: false,
            },
        )?;
        let _ = receive::<Answer>(&mut c.reader);
        drop(c);
        let until = Instant::now() + HAND_OVER_WAIT;
        loop {
            let answered = self.answering()?;
            if let Some(n) = answered {
                if n.named.pid != asked.pid || n.named.since != asked.since {
                    return Ok(n);
                }
                if Instant::now() >= until {
                    bail!(
                        "sluice's host process {} (pid {}) was asked to start afresh as {} and still answers as it was after {} seconds; see {}",
                        asked.version,
                        asked.pid,
                        self.program.display(),
                        HAND_OVER_WAIT.as_secs(),
                        self.paths.log().display()
                    );
                }
            } else if !alive(asked.pid) {
                return self.start();
            } else if Instant::now() >= until {
                bail!(
                    "sluice's host process {} (pid {}) was asked to hand over and has not ended or started afresh within {} seconds; see {}",
                    asked.version,
                    asked.pid,
                    HAND_OVER_WAIT.as_secs(),
                    self.paths.log().display()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `sluice hand-over` (the change `sluice-host-owns-its-workers`,
    /// design D5): have the host process that answers, where it runs
    /// another executable than this command's, start afresh in place as
    /// this command's, keeping its runs, and wait for the image started
    /// afresh. It starts no host process. The process asked and the one
    /// that answers after; `None` where none answers, or where it runs this
    /// command's executable already.
    pub fn hand_over_here(&self) -> Result<Option<(Named, Named)>> {
        let Some(mut c) = self.answering()? else {
            return Ok(None);
        };
        let program = self.program();
        if program.is_some() && c.named.program == program {
            return Ok(None);
        }
        let asked = c.named.clone();
        send(
            &mut c.writer,
            &Request::HandOver {
                program,
                ends: false,
            },
        )?;
        let _ = receive::<Answer>(&mut c.reader);
        drop(c);
        let until = Instant::now() + HAND_OVER_WAIT;
        loop {
            if let Some(n) = self.answering()? {
                if n.named.pid != asked.pid || n.named.since != asked.since {
                    return Ok(Some((asked, n.named)));
                }
            } else if !alive(asked.pid) {
                bail!(
                    "sluice's host process {} (pid {}) ended when asked to hand over; see {}",
                    asked.version,
                    asked.pid,
                    self.paths.log().display()
                );
            }
            if Instant::now() >= until {
                bail!(
                    "sluice's host process {} (pid {}) was asked to start afresh as {} and did not within {} seconds; see {}",
                    asked.version,
                    asked.pid,
                    self.program.display(),
                    HAND_OVER_WAIT.as_secs(),
                    self.paths.log().display()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Start the host process under the start lock, unless one answers
    /// once the lock is this command's, and wait for it to answer.
    fn start(&self) -> Result<Connection> {
        let lock = self.start_lock()?;
        if let Some(c) = self.answering()? {
            return Ok(c);
        }
        // The process keeps the log before its own as `log.1` once it holds
        // its lock; what it says before that goes to the last one's.
        let log = self.paths.log();
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .with_context(|| format!("opening {}", log.display()))?;
        let mut command = Command::new(&self.program);
        command
            .arg("host")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .env_clear()
            .envs(environment::kept(self.env.clone()));
        // SAFETY: setsid is async-signal-safe; nothing else runs here.
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        let started = command.spawn();
        let mut child = match started {
            Ok(c) => c,
            Err(e) => {
                drop(lock);
                bail!(
                    "sluice's host process could not be started from {}: {e}; see {}",
                    self.program.display(),
                    log.display()
                );
            }
        };
        // Reaped once it ends, so it never waits on this command.
        std::thread::spawn(move || child.wait());
        let until = Instant::now() + START_WAIT;
        loop {
            if let Some(c) = self.answering()? {
                return Ok(c);
            }
            if Instant::now() >= until {
                drop(lock);
                bail!(
                    "sluice's host process did not answer within {} seconds of its start; see {}",
                    START_WAIT.as_secs(),
                    log.display()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The start lock, once whatever holds it lets go.
    fn start_lock(&self) -> Result<File> {
        let path = self.paths.start_lock();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        file.lock()
            .with_context(|| format!("locking {}", path.display()))?;
        Ok(file)
    }

    /// Ask `request` of the host process, started where none answers, and
    /// read its one answer.
    pub fn ask(&self, request: &Request) -> Result<Answer> {
        let mut c = self.reach()?;
        c.ask(request)
    }

    /// A read's output (the change `sluice-host-answers-every-read`, design
    /// D1): the host process, started where none answers, answers
    /// `request` with what the command prints and its exit code. A host
    /// process that ends before it answers, or answers with an error, is
    /// an error, in its words.
    pub fn output(&self, request: &Request) -> Result<Output> {
        let mut c = self.reach()?;
        let named = c.named.clone();
        send(&mut c.writer, request)?;
        match receive::<Answer>(&mut c.reader) {
            Ok(Answer::Output(output)) => Ok(output),
            Ok(Answer::Error(words)) => bail!(
                "sluice's host process, sluice {} (pid {}), did not answer: {words}",
                named.version,
                named.pid
            ),
            Ok(other) => Err(unexpected(other)),
            Err(e) => bail!(
                "sluice's host process, sluice {} (pid {}), ended before it answered: {e:#}",
                named.version,
                named.pid
            ),
        }
    }

    /// A session's command (the change `sluice-host-runs-a-sessions-checks`,
    /// design D1): the host process, started where none answers, runs
    /// `request`; what it writes is printed as it comes, on standard output
    /// and standard error as the command wrote it, and its exit code is
    /// given. Ctrl-C, where [`crate::stop_on_interrupt`] took it, closes the
    /// connection's side of this command, which stops the command as
    /// Ctrl-C stopped it, and what the command says of its stop is still
    /// printed (design D4). A host process that ends before the command's
    /// `output`, or answers with an error, is an error, in its words.
    pub fn command(&self, request: &Request) -> Result<i32> {
        let c = self.reach()?;
        Client::command_on(c, request)
    }

    /// [`Client::command`] where a host process answers now, starting none:
    /// `None` where none answers, or its socket cannot be reached (record
    /// 0120 as amended 2026-11-17: a worktree's release with nothing to free
    /// starts no host process).
    pub fn command_if_answering(&self, request: &Request) -> Result<Option<i32>> {
        let Ok(Some(c)) = self.answering() else {
            return Ok(None);
        };
        Client::command_on(c, request).map(Some)
    }

    /// `request` asked on `c`, its lines printed as they come (design D1,
    /// D4).
    fn command_on(mut c: Connection, request: &Request) -> Result<i32> {
        use std::io::Write as _;
        let named = c.named.clone();
        send(&mut c.writer, request)?;
        let closing = c.writer.try_clone()?;
        std::thread::spawn(move || {
            while !crate::was_interrupted() {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = closing.shutdown(std::net::Shutdown::Write);
        });
        let mut out = std::io::stdout();
        loop {
            match receive::<Answer>(&mut c.reader) {
                Ok(Answer::Stdout(text)) => {
                    let _ = out.write_all(text.as_bytes());
                    let _ = out.flush();
                }
                Ok(Answer::Stderr(line)) => eprintln!("{line}"),
                Ok(Answer::Output(output)) => {
                    let _ = out.write_all(output.stdout.as_bytes());
                    let _ = out.flush();
                    for line in &output.stderr {
                        eprintln!("{line}");
                    }
                    return Ok(output.code);
                }
                Ok(Answer::Error(words)) => bail!(
                    "sluice's host process, sluice {} (pid {}), did not answer: {words}",
                    named.version,
                    named.pid
                ),
                Ok(other) => return Err(unexpected(other)),
                Err(e) => bail!(
                    "sluice's host process, sluice {} (pid {}), ended before the command did: {e:#}",
                    named.version,
                    named.pid
                ),
            }
        }
    }

    /// A submit's answer (the change `sluice-host-makes-each-change`,
    /// design D2): what it printed, and the wait it follows next, as
    /// `output` answers a read.
    pub fn submitted(&self, request: &Request) -> Result<super::protocol::Submitted> {
        let mut c = self.reach()?;
        let named = c.named.clone();
        send(&mut c.writer, request)?;
        match receive::<Answer>(&mut c.reader) {
            Ok(Answer::Submitted(submitted)) => Ok(submitted),
            Ok(Answer::Error(words)) => bail!(
                "sluice's host process, sluice {} (pid {}), did not answer: {words}",
                named.version,
                named.pid
            ),
            Ok(other) => Err(unexpected(other)),
            Err(e) => bail!(
                "sluice's host process, sluice {} (pid {}), ended before it answered: {e:#}",
                named.version,
                named.pid
            ),
        }
    }

    /// A wait's step of `ask`, from `folder` (the change
    /// `sluice-host-answers-every-read`, design D8), as `output` answers a
    /// read.
    pub fn wait(
        &self,
        folder: &Path,
        ask: &super::protocol::WaitAsk,
    ) -> Result<super::protocol::WaitStep> {
        let mut c = self.reach()?;
        let named = c.named.clone();
        let request = Request::Wait {
            folder: folder.to_path_buf(),
            caller: crate::caller::here(),
            wait: ask.clone(),
        };
        send(&mut c.writer, &request)?;
        match receive::<Answer>(&mut c.reader) {
            Ok(Answer::Waited(step)) => Ok(step),
            Ok(Answer::Error(words)) => bail!(
                "sluice's host process, sluice {} (pid {}), did not answer: {words}",
                named.version,
                named.pid
            ),
            Ok(other) => Err(unexpected(other)),
            Err(e) => bail!(
                "sluice's host process, sluice {} (pid {}), ended before it answered: {e:#}",
                named.version,
                named.pid
            ),
        }
    }

    /// Tell the host process `repository` has work (D2): it holds it and
    /// passes it.
    pub fn serve(&self, repository: &Path) -> Result<()> {
        match self.ask(&Request::Serve {
            repository: repository.to_path_buf(),
        })? {
            Answer::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// One round of `branch` of the repository that holds `folder`, as
    /// `sluice step --json` prints it, with what it said and its line
    /// (D10; the change `sluice-host-makes-each-change`, design D5).
    pub fn step(&self, folder: &Path, branch: &str) -> Result<serde_json::Value> {
        match self.ask(&Request::Step {
            folder: folder.to_path_buf(),
            branch: branch.to_string(),
        })? {
            Answer::Step(doc) => Ok(doc),
            other => Err(unexpected(other)),
        }
    }

    /// The results `ask` names of `repository`, as `branch`'s gate reads
    /// them, from the host process's index, started where none answers
    /// (D17).
    pub fn results(
        &self,
        repository: &Path,
        branch: &str,
        ask: &sluice_seams::ResultAsk,
    ) -> Result<Vec<u8>> {
        match self.ask(&Request::Results {
            repository: repository.to_path_buf(),
            branch: branch.to_string(),
            wants: ask.clone(),
        })? {
            Answer::Results(lines) => Ok(lines.into_bytes()),
            other => Err(unexpected(other)),
        }
    }

    /// A pass of the repository that holds `folder` now, once it ends, at
    /// the asker's time `at` (D14; design D5).
    pub fn pass(&self, folder: &Path, at: Option<chrono::DateTime<chrono::Utc>>) -> Result<Report> {
        match self.ask(&Request::Pass {
            folder: folder.to_path_buf(),
            at: at.map(|at| at.to_rfc3339()),
        })? {
            Answer::Pass(r) => Ok(r),
            other => Err(unexpected(other)),
        }
    }

    /// A subscription of `folders`, each followed as its kind says, the
    /// connection kept open for more `add` and `remove` lines and for the
    /// lines that the host process sends (the change
    /// `sluice-host-answers-every-read`, design D6).
    pub fn subscribe(&self, folders: Vec<super::protocol::Added>) -> Result<Connection> {
        let mut c = self.reach()?;
        send(&mut c.writer, &Request::Subscribe { folders })?;
        Ok(c)
    }
}

impl Connection {
    /// Ask `request` and read the one answer.
    pub fn ask(&mut self, request: &Request) -> Result<Answer> {
        send(&mut self.writer, request)?;
        receive(&mut self.reader)
    }

    /// The next answer on the connection, waiting up to `wait`
    /// where given: `None` where none came within it.
    pub fn next(&mut self, wait: Option<Duration>) -> Result<Option<Answer>> {
        self.reader.get_ref().set_read_timeout(wait)?;
        match receive::<Answer>(&mut self.reader) {
            Ok(a) => Ok(Some(a)),
            Err(e) => {
                let timed_out = e.downcast_ref::<std::io::Error>().is_some_and(|io| {
                    matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                });
                match timed_out {
                    true => Ok(None),
                    false => Err(e),
                }
            }
        }
    }

    /// The next line on a subscription, waiting up to `wait` where given:
    /// `None` where none came within it (design D6).
    pub fn line(
        &mut self,
        wait: Option<Duration>,
    ) -> Result<Option<super::protocol::SubscriptionLine>> {
        self.reader.get_ref().set_read_timeout(wait)?;
        match receive(&mut self.reader) {
            Ok(line) => Ok(Some(line)),
            Err(e) => {
                let timed_out = e.downcast_ref::<std::io::Error>().is_some_and(|io| {
                    matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                });
                match timed_out {
                    true => Ok(None),
                    false => Err(e),
                }
            }
        }
    }

    /// A second handle of the connection's stream, which writes the lines
    /// of a subscription from another thread and shuts it down (design
    /// D7).
    pub fn stream(&self) -> Result<UnixStream> {
        Ok(self.writer.try_clone()?)
    }

    /// Send `ask` on a subscription: an `add` or a `remove` line (design
    /// D6).
    pub fn say(&mut self, ask: &super::protocol::SubscriptionAsk) -> Result<()> {
        send(&mut self.writer, ask)
    }
}

/// An answer the request does not take.
fn unexpected(answer: Answer) -> anyhow::Error {
    match answer {
        Answer::Error(words) => anyhow!("{words}"),
        other => anyhow!("sluice's host process answered {other:?}"),
    }
}
