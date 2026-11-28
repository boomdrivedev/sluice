//! Branch protection on one host: a `reference-transaction` hook refuses
//! any update to a branch the policy names that sluice did not make, and
//! reports it in the branch's records (1, 124; C.4 local refs; rule
//! core.4; `docs/plans/phase-1-integrations.md` section 2).
//!
//! A refusal is recorded once per cause, the command that tried it on
//! this host with each full revision in it read as `<revision>`: its
//! first is a `protected` line in the branch's log, and every later one
//! of a cause that stands is counted in the branch's state, with no line
//! (148; rule core.47). What the caller is told of each refusal is the
//! same, the first or the thousandth.
//!
//! The hook asks `sluice host`, which judges each transaction from the
//! state that it keeps and writes each refusal; the hook reads no file of
//! the repository, no record and none of the host's settings, starts a
//! `sluice host` where none answers, and refuses the write where none
//! answers within ten seconds (the change `the-git-hook-asks-sluice-host`,
//! design D1, D2). A git command of `sluice host`'s own carries its name,
//! and the hook lets its transaction go with no ask, so a fetch of
//! `sluice host` whose packing runs the hook waits on nothing (record
//! 0110; design D3).
//!
//! `sluice host` judges a write by the value the branch's ref holds on
//! this host, never by a fetch of its remote: for a branch kept on a
//! remote that ref is sluice's copy, which only sluice's rounds level
//! (124). Judging fetches nothing: git's own packing passes before any
//! record is read, and what still needs a branch is read from its records
//! as last fetched (S64). It judges the repository git names, wherever
//! the command ran, and lets through, said once, a transaction it cannot
//! read for a reason of its own.
//!
//! It reads of the host's settings and the policy only what guarding
//! needs, and passes over every other field, known to it or not. The hook
//! runs the sluice that installed it, by its path, and `sluice host` puts
//! it back for itself, by its own path, in each repository it binds for a
//! request, where it stands for another (124; ADR 0059; design D4).
//!
//! sluice moves branches through gix, which runs no hooks, so its own
//! moves never reach the hook. Everything that runs git does. This guards
//! against sessions running git; it is no security boundary.

use crate::workers::Run as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use sluice_core::Sluice;
use sluice_seams::{HostCopy, Rev};

use crate::exec::Host;

/// The line that marks a hook as sluice's.
const MARKER: &str =
    "# sluice: refuses updates to branches sluice protects (docs: branch protection). Installed by sluice.";
const HOOK: &str = "reference-transaction";
const CHAINED: &str = "reference-transaction.pre-sluice";

/// One write of a hooks folder at a time in this process. `sluice host`
/// puts back or installs the hook for each request, and for a `sluice
/// protect` beside them, on its pool for blocking work: two at once would
/// write one temporary file, which the first moves into place and the
/// second then cannot find, or both move the hook found there aside, so
/// the second moves sluice's over the hook the first kept (124).
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// This process's turn to write a hooks folder.
fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = crate::workers::command("git", "git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .stdin(Stdio::null())
        .worker_output()
        .context("running git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// The repository's hook directory, as git finds it from `dir`: it
/// follows `core.hooksPath`. A hook directory inside a worktree is a
/// tracked one, which sluice will not write.
fn hookdir(dir: &Path) -> Result<PathBuf> {
    match located(dir)? {
        (hooks, None) => Ok(hooks),
        (hooks, Some(worktree)) => bail!(
            "core.hooksPath is {}, inside the worktree {}: sluice will not write a tracked hook; \
             add `{} hook {HOOK} \"$1\"` to that hook yourself",
            hooks.display(),
            worktree.display(),
            std::env::current_exe()?.display()
        ),
    }
}

/// The repository's hook directory, as git finds it from `dir`, and the
/// worktree it is inside, where it is a tracked one.
fn located(dir: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let hooks = PathBuf::from(git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?);
    let common = PathBuf::from(git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    if hooks.starts_with(&common) {
        return Ok((hooks, None));
    }
    let list = git(dir, &["worktree", "list", "--porcelain"])?;
    let mut path = None;
    for line in list.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line == "bare" {
            path = None;
        } else if line.is_empty() {
            if let Some(p) = path.take() {
                if hooks.starts_with(&p) {
                    return Ok((hooks, Some(p)));
                }
            }
        }
    }
    Ok((hooks, None))
}

fn shim(program: &Path) -> String {
    let p = program.display().to_string().replace('\'', "'\\''");
    format!(
        "#!/bin/sh\n{MARKER}\ninput=$(cat)\nd=$(dirname \"$0\")\n\
         if [ -x '{p}' ]; then\n\
         \x20 printf '%s\\n' \"$input\" | '{p}' hook {HOOK} \"$1\" || exit $?\n\
         else\n\
         \x20 echo \"warning: sluice's branch protection is installed but {p} is missing\" >&2\n\
         fi\n\
         [ -x \"$d/{CHAINED}\" ] || exit 0\n\
         printf '%s\\n' \"$input\" | \"$d/{CHAINED}\" \"$@\"\n"
    )
}

/// The hooks folder of the repository holding `dir`, as git finds it:
/// an error where it is a tracked one, which sluice will not write, or
/// where `dir` is in no repository.
pub fn hooks_of(dir: &Path) -> Result<PathBuf> {
    hookdir(dir)
}

/// Install sluice's hook in the repository holding `dir`, for `program`,
/// keeping any hook already there by running it after sluice's. Writes
/// nothing when sluice's hook for `program` is already in place, and says
/// so with `false`.
pub fn install(dir: &Path, program: &Path) -> Result<bool> {
    install_at(&hookdir(dir)?, program)
}

/// [`install`], in the hooks folder `hooks`.
pub fn install_at(hooks: &Path, program: &Path) -> Result<bool> {
    let _turn = writing();
    std::fs::create_dir_all(hooks)?;
    let hook = hooks.join(HOOK);
    let want = shim(program);
    match std::fs::read_to_string(&hook) {
        Ok(have) if have == want => return Ok(false),
        Ok(have) if have.lines().nth(1) == Some(MARKER) => {}
        Ok(_) => {
            let chained = hooks.join(CHAINED);
            if chained.exists() {
                bail!(
                    "{} and {} both exist: sluice will not replace either",
                    hook.display(),
                    chained.display()
                );
            }
            std::fs::rename(&hook, &chained)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    write(hooks, &want)?;
    Ok(true)
}

/// 124: put sluice's hook for `program` where the hook in place is
/// sluice's for another, keeping the hook it chains, so a host whose
/// sluice was upgraded is guarded by the newer one from its next request
/// in the repository. Writes nothing where there is no hook, where it is
/// someone else's or already `program`'s, or where the hooks directory is
/// tracked: a hook is installed only by `sluice protect` and the requests
/// that run sluice's work (ADR 0059). `true` when it wrote the hook.
pub fn reinstall(dir: &Path, program: &Path) -> Result<bool> {
    // No repository to find a hook in is the command's to report, and a
    // tracked hooks directory holds no hook sluice wrote.
    let Ok((hooks, None)) = located(dir) else {
        return Ok(false);
    };
    reinstall_at(&hooks, program)
}

/// [`reinstall`], in the hooks folder `hooks`.
pub fn reinstall_at(hooks: &Path, program: &Path) -> Result<bool> {
    let _turn = writing();
    let hook = hooks.join(HOOK);
    let want = shim(program);
    match std::fs::read_to_string(&hook) {
        Ok(have) if have != want && have.lines().nth(1) == Some(MARKER) => {
            write(hooks, &want)?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", hook.display())),
    }
}

/// Write `shim` as the hook in `hooks`, in one rename.
fn write(hooks: &Path, shim: &str) -> Result<()> {
    let tmp = hooks.join(format!(".{HOOK}.sluice.{}", std::process::id()));
    std::fs::write(&tmp, shim)?;
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::rename(&tmp, hooks.join(HOOK))?;
    Ok(())
}

/// Take sluice's hook out, putting back the hook it chained; `false` when
/// it was not in.
pub fn remove(dir: &Path) -> Result<bool> {
    let hooks = hookdir(dir)?;
    let _turn = writing();
    let hook = hooks.join(HOOK);
    match std::fs::read_to_string(&hook) {
        Ok(have) if have.lines().nth(1) == Some(MARKER) => {
            std::fs::remove_file(&hook)?;
            let chained = hooks.join(CHAINED);
            if chained.exists() {
                std::fs::rename(&chained, &hook)?;
            }
            Ok(true)
        }
        Ok(_) => bail!("{} is not sluice's hook", hook.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// A write the hook refuses: the branch, the value it holds on this host,
/// the one the write would set or none for a deletion, the submissions
/// that still need it, and whether it is sluice's copy of a branch kept
/// on a remote.
struct Refused {
    branch: String,
    from: Rev,
    to: Option<Rev>,
    needing: Vec<u64>,
    copy: bool,
}

/// What a refused update is told, on stderr, before git's own error.
fn refusal(r: &Refused, by: &str) -> String {
    let branch = &r.branch;
    let mut said =
        format!("error: {branch} is protected by sluice; submit the change to sluice instead\n");
    match &r.to {
        Some(to) => said.push_str(&format!(
            "  `{by}` would move it from {} to {}\n",
            r.from.short(),
            to.short()
        )),
        None => {
            let ids: Vec<String> = r.needing.iter().map(|i| format!("#{i}")).collect();
            said.push_str(&format!(
                "  `{by}` would delete it at {}; it still holds {}: withdraw or land them first\n",
                r.from.short(),
                ids.join(", ")
            ));
        }
    }
    if r.copy {
        said.push_str(&format!(
            "  sluice levels this host's {branch} to each landing itself\n"
        ));
    }
    said.push_str(
        "  the worktree may hold the attempted change: git merge --abort or git reset --hard",
    );
    said
}

/// 124: what the hook says when it lets a transaction through unread.
pub fn unread(why: &anyhow::Error) -> String {
    format!("warning: sluice's branch protection let this through unread: {why:#}")
}

/// The command that ran git: the hook is sluice's parent's child, and git
/// the shim's parent. Read from the kernel, never by running `ps`, which
/// a sandbox refuses to start (it is setuid on macOS).
pub fn caller() -> String {
    let shim = nix::unistd::getppid().as_raw();
    parent_of(shim)
        .and_then(command_of)
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(target_os = "macos")]
fn parent_of(pid: i32) -> Option<i32> {
    use nix::libc;
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: `info` is a `proc_bsdinfo` of `size` bytes, as the call asks.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then_some(info.pbi_ppid as i32)
}

/// The process's arguments, from `KERN_PROCARGS2`: its argument count,
/// the executable's path, padding, then each argument.
#[cfg(target_os = "macos")]
fn command_of(pid: i32) -> Option<String> {
    use nix::libc;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: usize = 0;
    // SAFETY: a null buffer asks only for the size.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` holds `size` bytes, as the call asks.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size < 4 {
        return None;
    }
    buf.truncate(size);
    let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
    let rest = &buf[4..];
    let path_end = rest.iter().position(|b| *b == 0)?;
    let args: Vec<String> = rest[path_end..]
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    (!args.is_empty()).then(|| args.join(" "))
}

#[cfg(not(target_os = "macos"))]
fn parent_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command is in parentheses and may hold spaces: the parent's id is
    // the second field after the last `)`.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(not(target_os = "macos"))]
fn command_of(pid: i32) -> Option<String> {
    let args = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = args
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    (!args.is_empty()).then(|| args.join(" "))
}

/// Whether deleting `branch`, at `cur`, leaves it at `cur`: git's
/// `pack-refs` writes the branch into `packed-refs` at the value it holds,
/// then prunes the loose file in a transaction that leaves `packed-refs`
/// alone. Every other deletion locks `packed-refs` before the hook runs,
/// to take the branch out of it. `common` is the repository's common git
/// directory.
fn pruned(common: &Path, branch: &str, cur: &Rev) -> Result<bool> {
    if common.join("packed-refs.lock").exists() {
        return Ok(false);
    }
    let packed = match std::fs::read_to_string(common.join("packed-refs")) {
        Ok(packed) => packed,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let name = format!("refs/heads/{branch}");
    Ok(packed
        .lines()
        .filter_map(|l| l.split_once(' '))
        .any(|(id, n)| n == name && id == cur.as_str()))
}

/// The variable that names the `sluice host` whose git commands carry it,
/// `<pid> <start>`: its pid and its start time in seconds (the change
/// `the-git-hook-asks-sluice-host`, design D3). The hook lets a
/// transaction whose git carries the name of a live `sluice host` go with
/// no ask (record 0110).
pub const HOST_VAR: &str = "SLUICE_HOST_PROCESS";

/// How long the hook waits for `sluice host`'s answer, its start or a
/// hand-over's wait for the rounds to end among it, before it refuses the
/// transaction (design D2).
pub const ASK_FOR: Duration = Duration::from_secs(10);

/// How long `sluice host` waits for a branch's lock to write a refusal of
/// a write to it, within the hook's [`ASK_FOR`]: a round of the branch
/// holds its lock while it lands or starts its runs (148).
pub const RECORD_WAIT: Duration = Duration::from_secs(5);

/// The value of [`HOST_VAR`] for the process `pid` that started at
/// `start`.
pub fn host_name(pid: u32, start: u64) -> String {
    format!("{pid} {start}")
}

/// Name this process, `sluice host`, in [`HOST_VAR`] in its own
/// environment, so each git command it starts carries it, whatever starts
/// it (design D3); where its start time cannot be read, it names none.
/// Called before any thread starts.
pub fn name_this_host() {
    let pid = std::process::id();
    match crate::workers::started(pid as i32) {
        Some(start) => std::env::set_var(HOST_VAR, host_name(pid, start)),
        None => std::env::remove_var(HOST_VAR),
    }
}

/// Whether `value`, as [`HOST_VAR`] holds it, names a `sluice host` that
/// runs: its pid runs and started at the time it names (design D3). A
/// value an ended `sluice host` gave names none, whatever runs at its pid
/// now.
pub fn names_a_live_host(value: &str) -> bool {
    let mut parts = value.split_whitespace();
    let (Some(pid), Some(start), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(pid), Ok(start)) = (pid.parse::<i32>(), start.parse::<u64>()) else {
        return false;
    };
    pid > 0 && crate::workers::started(pid) == Some(start)
}

/// What the hook says where `sluice host` did not judge the transaction
/// in time, or could not be reached: the write is refused (design D2).
pub fn no_answer(why: Option<&anyhow::Error>) -> String {
    let mut said = match why {
        Some(e) => format!(
            "error: sluice's host process did not answer, so sluice's branch protection refuses this write: {e:#}\n"
        ),
        None => format!(
            "error: sluice's host process did not answer within {} seconds, so sluice's branch protection refuses this write\n",
            ASK_FOR.as_secs()
        ),
    };
    said.push_str(
        "  `sluice protect --remove` takes sluice's branch protection out of this repository",
    );
    said
}

/// `sluice hook reference-transaction prepared`, with the transaction
/// `lines` read whole from its standard input: let a transaction whose git
/// carries the name of a live `sluice host` go, and otherwise ask `sluice
/// host` whether it goes, starting one where none answers, and say what it
/// says (design D1 to D3). Returns the exit: 0 allows, 1 refuses. Where
/// `sluice host` does not answer within [`ASK_FOR`], the transaction is
/// refused. It reads no file of the repository, no record and none of the
/// host's settings.
pub fn ask(lines: String) -> i32 {
    if std::env::var(HOST_VAR).is_ok_and(|v| names_a_live_host(&v)) {
        return 0;
    }
    let folder = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{}", no_answer(Some(&anyhow::Error::from(e))));
            return 1;
        }
    };
    let git_dir = std::env::var_os("GIT_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let request = crate::host_process::protocol::Request::ReferenceTransaction {
        folder,
        git_dir,
        by: caller(),
        lines,
    };
    let (send, answered) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = crate::host_process::client::Client::host().and_then(|c| c.output(&request));
        let _ = send.send(output);
    });
    match answered.recv_timeout(ASK_FOR) {
        Ok(Ok(output)) => {
            print!("{}", output.stdout);
            for line in &output.stderr {
                eprintln!("{line}");
            }
            output.code
        }
        Ok(Err(e)) => {
            eprintln!("{}", no_answer(Some(&e)));
            1
        }
        Err(_) => {
            eprintln!("{}", no_answer(None));
            1
        }
    }
}

/// Judge the transaction `text`, in git's `prepared` state, that `by` ran:
/// refuse any update to a protected branch that is not a creation or
/// leaves its value on this host as it was (1, 124), as `sluice host`
/// judges what the hook sends (design D1). Returns the exit: 0 allows, 1
/// refuses. A transaction it cannot read for a reason of its own passes,
/// said once. Each refusal is said, then recorded under the branch's lock:
/// logged where its cause does not stand, and otherwise counted with its
/// cause (148). One whose lock does not come within [`RECORD_WAIT`] is said
/// and recorded nowhere.
pub fn judged(s: &Sluice<'_>, host: &dyn Host, text: &str, by: &str) -> i32 {
    let refused = match judge(s, &|| host.repository(), text) {
        Ok(refused) => refused,
        Err(e) => {
            crate::telemetry::say(&unread(&e));
            return 0;
        }
    };
    if refused.is_empty() {
        return 0;
    }
    for r in &refused {
        crate::telemetry::warn(&refusal(r, by));
    }
    let seams = s.seams();
    for r in &refused {
        // Reported when the branch's lock comes within its wait, which a
        // round of the branch may hold meanwhile; the refusal stands
        // either way.
        let until = Instant::now() + RECORD_WAIT;
        let guard = loop {
            match seams.state.lock(&r.branch) {
                Ok(Some(g)) => break Some(g),
                Ok(None) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => break None,
                Err(e) => {
                    crate::telemetry::say(&format!(
                        "warning: locking {}'s records: {e:#}",
                        r.branch
                    ));
                    break None;
                }
            }
        };
        if guard.is_some() {
            if let Err(e) = s.record_protected(&r.branch, &r.from, r.to.as_ref(), by) {
                crate::telemetry::say(&format!(
                    "warning: reporting the refused update to {}: {e:#}",
                    r.branch
                ));
            }
        }
    }
    1
}

/// The writes of a transaction the hook refuses, each judged by the
/// branch's own value on this host; none fetches. `repository` names the
/// repository's common git directory, read only for a deletion, where
/// git's own packing is told apart.
fn judge(
    s: &Sluice<'_>,
    repository: &dyn Fn() -> Result<String>,
    text: &str,
) -> Result<Vec<Refused>> {
    let seams = s.seams();
    let mut refused = Vec::new();
    for line in text.lines() {
        let mut f = line.split(' ');
        let (Some(_old), Some(new), Some(name)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let Some(branch) = name.strip_prefix("refs/heads/") else {
            continue;
        };
        // The caller's expected value is often all zeros; the ref as it
        // stands is still the old one while the transaction is prepared.
        let (cur, copy) = match seams.branch.copy(branch)? {
            HostCopy::At(at) => (at, true),
            HostCopy::Itself => (seams.branch.read(branch)?, false),
        };
        let Some(cur) = cur else {
            continue; // a creation
        };
        let deleting = new.bytes().all(|b| b == b'0');
        let new = (!deleting).then(|| Rev::new(new));
        if new.as_ref() == Some(&cur) || !s.protects(branch, &cur)? {
            continue;
        }
        let needing = if deleting {
            // git's own packing, which a fetch's maintenance runs, is let
            // through before the records are read (S64; record 0110).
            if pruned(Path::new(&repository()?), branch, &cur)? {
                continue;
            }
            let needing = s.needing(branch)?;
            if needing.is_empty() {
                continue;
            }
            needing
        } else {
            Vec::new()
        };
        refused.push(Refused {
            branch: branch.to_string(),
            from: cur,
            to: new,
            needing,
            copy,
        });
    }
    Ok(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 124: installs of one hooks folder at once, as `sluice host` makes
    /// for a subscription and a `sluice protect` beside it, each end well,
    /// and the hook that was there before is kept, chained.
    #[test]
    fn installs_at_once_each_end_and_keep_the_hook_there() {
        let base = std::env::temp_dir().join(format!(
            "sluice-hooks-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let program = Path::new("/bin/sluice");
        for round in 0..20 {
            let hooks = base.join(round.to_string());
            std::fs::create_dir_all(&hooks).unwrap();
            std::fs::write(hooks.join(HOOK), "#!/bin/sh\n# the person's\n").unwrap();
            let start = std::sync::Arc::new(std::sync::Barrier::new(8));
            let installs: Vec<_> = (0..8)
                .map(|_| {
                    let (hooks, start) = (hooks.clone(), start.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        install_at(&hooks, program).map_err(|e| format!("{e:#}"))
                    })
                })
                .collect();
            for install in installs {
                install.join().unwrap().unwrap();
            }
            assert_eq!(
                std::fs::read_to_string(hooks.join(HOOK)).unwrap(),
                shim(program)
            );
            assert_eq!(
                std::fs::read_to_string(hooks.join(CHAINED)).unwrap(),
                "#!/bin/sh\n# the person's\n"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `bolt/x` under a policy that protects it, and a session's head off
    /// it: the fakes, its revision and the head.
    fn protected() -> (sluice_fakes::Fakes, Rev, Rev) {
        let f = sluice_fakes::Fakes::new();
        let tip = f.world.init(
            "bolt/x",
            &[(
                ".config/sluice.toml",
                "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\n",
            )],
        );
        f.world.task("app:test", "");
        let head = f.world.work(&tip, &[("a.txt", Some("a"))]);
        f.world.session("unit/a", "agent-a", &head);
        (f, tip, head)
    }

    /// git's transaction line moving `name` from `from` to `to`.
    fn update(from: &str, to: &str, name: &str) -> String {
        format!("{from} {to} {name}\n")
    }

    const ZERO: &str = "0000000000000000000000000000000000000000";

    /// What the protection refuses of `text`, the repository's common
    /// directory `common`.
    fn judging(f: &sluice_fakes::Fakes, common: &Path, text: &str) -> Vec<Refused> {
        let common = common.to_string_lossy().into_owned();
        judge(&f.sluice(), &|| Ok(common.clone()), text).unwrap()
    }

    /// A directory standing for the repository's common directory, with
    /// no `packed-refs`, removed once dropped.
    struct Common(PathBuf);

    impl Common {
        fn new(name: &str) -> Common {
            let dir = std::env::temp_dir().join(format!("sluice-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Common(dir)
        }
    }

    impl Drop for Common {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 1, S1, rule core.4: a move of a protected branch is refused, told
    /// before git's own error that the branch is protected, the command,
    /// both revisions and what the worktree may hold; a branch in this
    /// host's own refs is told no line of a copy.
    #[test]
    fn a_move_of_a_protected_branch_is_refused_saying_the_command() {
        let (f, tip, head) = protected();
        let common = Common::new("protect-move");
        let refused = judging(
            &f,
            &common.0,
            &update(tip.as_str(), head.as_str(), "refs/heads/bolt/x"),
        );
        assert_eq!(refused.len(), 1);
        let r = &refused[0];
        assert_eq!(
            (r.branch.as_str(), &r.from, r.to.as_ref()),
            ("bolt/x", &tip, Some(&head))
        );
        assert!(!r.copy && r.needing.is_empty());
        let by = format!("git update-ref refs/heads/bolt/x {head}");
        let said = refusal(r, &by);
        assert!(
            said.starts_with(
                "error: bolt/x is protected by sluice; submit the change to sluice instead\n"
            ),
            "{said}"
        );
        assert!(
            said.contains(&format!(
                "`{by}` would move it from {} to {}",
                tip.short(),
                head.short()
            )),
            "{said}"
        );
        assert!(
            said.ends_with(
                "the worktree may hold the attempted change: git merge --abort or git reset --hard"
            ),
            "{said}"
        );
        assert!(!said.contains("levels this host's"), "{said}");
    }

    /// What moves no protected branch passes: a write that leaves it where
    /// it is, a branch's making, a branch the policy does not name, and a
    /// ref that is no branch.
    #[test]
    fn what_moves_no_protected_branch_passes() {
        let (f, tip, head) = protected();
        f.world.set_branch("unit/a", &head);
        let common = Common::new("protect-pass");
        let text = [
            update(tip.as_str(), tip.as_str(), "refs/heads/bolt/x"),
            update(ZERO, head.as_str(), "refs/heads/bolt/new"),
            update(head.as_str(), tip.as_str(), "refs/heads/unit/a"),
            update(ZERO, head.as_str(), "refs/tags/v1"),
        ]
        .concat();
        assert!(judging(&f, &common.0, &text).is_empty());
    }

    /// 56: a branch whose own revision's policy does not name it is not
    /// protected, whatever another branch's policy says.
    #[test]
    fn the_branchs_own_revision_decides() {
        let (f, tip, _) = protected();
        let bare = f.world.work(&tip, &[(".config/sluice.toml", None)]);
        f.world.set_branch("bolt/z", &bare);
        let common = Common::new("protect-own");
        let text = update(bare.as_str(), tip.as_str(), "refs/heads/bolt/z");
        assert!(judging(&f, &common.0, &text).is_empty());
    }

    /// A deletion is refused while a submission still needs the branch,
    /// naming it, and passes once none does; git's own packing, which
    /// writes the branch into `packed-refs` at its value and prunes its
    /// loose file, passes all the same, and one beside a lock on
    /// `packed-refs`, as every other deletion takes, does not (S64; record
    /// 0110).
    #[test]
    fn a_branch_still_needed_is_not_deleted_but_git_may_pack_it() {
        use sluice_core::{Requester, SubmitHead, SubmitRequest};
        let (f, tip, _) = protected();
        let s = f.sluice();
        let id = s
            .submit(&SubmitRequest {
                head: SubmitHead::Worktree("unit/a".into()),
                branch: "bolt/x".into(),
                after: Vec::new(),
                priority: 0,
                subject: None,
                trace: None,
            })
            .unwrap()
            .expect("accepted")
            .id;
        let common = Common::new("protect-delete");
        let delete = update(tip.as_str(), ZERO, "refs/heads/bolt/x");
        let refused = judging(&f, &common.0, &delete);
        assert_eq!(refused.len(), 1);
        assert_eq!(
            (refused[0].to.as_ref(), &refused[0].needing[..]),
            (None, &[id][..])
        );
        let said = refusal(&refused[0], "git branch -D bolt/x");
        assert!(
            said.contains(&format!(
                "`git branch -D bolt/x` would delete it at {}; it still holds #{id}: \
                 withdraw or land them first",
                tip.short()
            )),
            "{said}"
        );

        std::fs::write(
            common.0.join("packed-refs"),
            format!("# pack-refs with: peeled fully-peeled sorted\n{tip} refs/heads/bolt/x\n"),
        )
        .unwrap();
        assert!(judging(&f, &common.0, &delete).is_empty(), "git's packing");
        std::fs::write(common.0.join("packed-refs.lock"), "").unwrap();
        assert_eq!(judging(&f, &common.0, &delete).len(), 1);
        std::fs::remove_file(common.0.join("packed-refs.lock")).unwrap();
        std::fs::remove_file(common.0.join("packed-refs")).unwrap();

        s.withdraw(id, &Requester::Operator).unwrap().unwrap();
        assert!(judging(&f, &common.0, &delete).is_empty());
    }

    /// 124: on a branch kept on a remote, a write is judged by this host's
    /// own copy: one that leaves the copy where it stands passes, one that
    /// moves it is refused, told that sluice levels the copy itself.
    #[test]
    fn a_copy_is_judged_by_its_own_value() {
        let (f, tip, head) = protected();
        f.world.on_remote("bolt/x");
        let moved = f.world.work(&tip, &[("b.txt", Some("b"))]);
        f.world.set_branch("bolt/x", &moved);
        f.world.set_copy("bolt/x", Some(&tip));
        let common = Common::new("protect-copy");
        let stays = update(tip.as_str(), tip.as_str(), "refs/heads/bolt/x");
        assert!(judging(&f, &common.0, &stays).is_empty());
        let refused = judging(
            &f,
            &common.0,
            &update(tip.as_str(), head.as_str(), "refs/heads/bolt/x"),
        );
        assert_eq!(refused.len(), 1);
        assert!(refused[0].copy);
        assert_eq!(refused[0].from, tip);
        let said = refusal(&refused[0], "git reset --hard");
        assert!(
            said.contains("sluice levels this host's bolt/x to each landing itself"),
            "{said}"
        );
    }
}
