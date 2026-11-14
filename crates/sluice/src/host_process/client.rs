//! How a command reaches sluice's host process (135; design D2, D8): it
//! connects to the socket and says `hello` with its version. Where nothing
//! answers, it takes `host/start.lock`, tries once more, and starts the
//! process detached: in a session of its own, working in `/`, reading
//! nothing, its output in `host/log` and the previous process's kept as
//! `log.1`, with the host's own environment (D3). It waits up to ten
//! seconds for it to answer, and otherwise fails naming the log. Ten
//! commands at once take the start lock in turn: the first starts the
//! process, and the others find it answering. A command newer than the
//! process that answers has it hand over once, waits for its socket to go,
//! and starts its own; an older one uses the process as it is.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, ErrorKind};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use sluice_seams::RunKind;

use super::pass::Report;
use super::protocol::{
    receive, send, Answer, FollowKind, Hello, Named, Reading, Request, RoomView, Started,
};
use super::{environment, Paths};
use crate::reader::older;

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
    /// handed over once for this newer command's own (D8).
    pub fn reach(&self) -> Result<Connection> {
        if let Some(c) = self.answering()? {
            if !older(&c.named.version, &self.version) {
                return Ok(c);
            }
            self.hand_over(c)?;
        }
        self.start()
    }

    /// Have the older process `c` hand over, and wait for it to end, which
    /// lets its lock go for the process this command starts: as long as it
    /// gives its tasks to finish their rounds, and a start's wait after
    /// ([`HAND_OVER_WAIT`]).
    fn hand_over(&self, mut c: Connection) -> Result<()> {
        send(&mut c.writer, &Request::HandOver)?;
        let _ = receive::<Answer>(&mut c.reader);
        let until = Instant::now() + HAND_OVER_WAIT;
        while alive(c.named.pid) {
            if Instant::now() >= until {
                bail!(
                    "sluice's host process {} (pid {}) was asked to hand over and has not ended within {} seconds; see {}",
                    c.named.version,
                    c.named.pid,
                    HAND_OVER_WAIT.as_secs(),
                    self.paths.log().display()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
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

    /// One round of `branch` of `repository`, as `sluice step --json`
    /// prints it, with what it said and its line (D10).
    pub fn step(&self, repository: &Path, branch: &str) -> Result<serde_json::Value> {
        match self.ask(&Request::Step {
            repository: repository.to_path_buf(),
            branch: branch.to_string(),
        })? {
            Answer::Step(doc) => Ok(doc),
            other => Err(unexpected(other)),
        }
    }

    /// A reading of `repository`, once one is current, started where none
    /// answers (D16): waited for the round's retry bound and a tick, then
    /// answered as not current, the command reading as the host holds it.
    pub fn read(&self, repository: &Path) -> Result<Reading> {
        let mut c = self.reach()?;
        send(
            &mut c.writer,
            &Request::Read {
                repository: repository.to_path_buf(),
            },
        )?;
        let wait = super::read_for() + super::READ_TICK;
        match c.next(Some(wait))? {
            Some(Answer::Read(r)) => Ok(r),
            Some(other) => Err(unexpected(other)),
            None => Ok(Reading::default()),
        }
    }

    /// A pass of `repository` now, once it ends (D14).
    pub fn pass(&self, repository: &Path) -> Result<Report> {
        match self.ask(&Request::Pass {
            repository: repository.to_path_buf(),
        })? {
            Answer::Pass(r) => Ok(r),
            other => Err(unexpected(other)),
        }
    }

    /// Follow `repository`'s passes while the connection is open (D7,
    /// D11): each pass's report comes down it.
    pub fn follow(&self, repository: &Path, kind: FollowKind) -> Result<Connection> {
        let mut c = self.reach()?;
        send(
            &mut c.writer,
            &Request::Follow {
                repository: repository.to_path_buf(),
                kind,
            },
        )?;
        Ok(c)
    }

    /// Ask for a session's or a preview's room (D12), the connection kept
    /// while it waits and while it is held.
    pub fn hold(&self, kind: RunKind, repository: &Path, place: &str) -> Result<Connection> {
        let mut c = self.reach()?;
        send(
            &mut c.writer,
            &Request::Hold {
                kind,
                repository: repository.to_path_buf(),
                place: place.to_string(),
            },
        )?;
        Ok(c)
    }

    /// The room as the process that answers holds it; `None` where none
    /// answers, and nothing is started.
    pub fn room(&self) -> Result<Option<(Named, RoomView)>> {
        let Some(mut c) = self.answering()? else {
            return Ok(None);
        };
        match c.ask(&Request::Room)? {
            Answer::Room(r) => Ok(Some((c.named, r))),
            other => Err(unexpected(other)),
        }
    }
}

impl Connection {
    /// Ask `request` and read the one answer.
    pub fn ask(&mut self, request: &Request) -> Result<Answer> {
        send(&mut self.writer, request)?;
        receive(&mut self.reader)
    }

    /// The next answer on a follow or a held room, waiting up to `wait`
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

    /// Say a held room's run began, in `place` with `run_dir` (D12).
    pub fn started(&mut self, place: &str, run_dir: &Path) -> Result<()> {
        send(
            &mut self.writer,
            &Started {
                place: place.to_string(),
                run_dir: run_dir.to_path_buf(),
            },
        )
    }
}

/// An answer the request does not take.
fn unexpected(answer: Answer) -> anyhow::Error {
    match answer {
        Answer::Error(words) => anyhow!("{words}"),
        other => anyhow!("sluice's host process answered {other:?}"),
    }
}
