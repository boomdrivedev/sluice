//! The caller's session, as a read names it to `sluice host` (the change
//! `sluice-host-answers-every-read`, design D1): the variables a driver
//! reads to find who asks, the agent's name and the pane it runs in, and
//! the trace context it was started under. `sluice host` answers a read,
//! and runs a session's command, on a thread of its own, so the variables
//! that the request names stand for the host process's own while its call
//! runs on that thread; and the folder that a session's command names
//! stands for the host process's own working folder (the change
//! `sluice-host-runs-a-sessions-checks`, design D1).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The variables a request takes from its caller's environment: its
/// session, and the trace context its command nests in (153).
pub const SESSION: [&str; 4] = [
    "SLUICE_AGENT",
    "HERDR_PANE_ID",
    "HERDR_SOCKET_PATH",
    "TRACEPARENT",
];

thread_local! {
    /// The caller's variables while a read's call runs on this thread.
    static CALLER: RefCell<Option<BTreeMap<String, String>>> = const { RefCell::new(None) };
    /// The caller's folder while a session's command runs on this thread.
    static FOLDER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// `name`, as the caller's environment holds it: the read's where a read's
/// call runs on this thread, else this process's own.
pub fn var(name: &str) -> Option<String> {
    let named = CALLER.with(|c| c.borrow().as_ref().map(|vars| vars.get(name).cloned()));
    match named {
        Some(value) => value,
        None => std::env::var(name).ok(),
    }
}

/// The session variables of this process's environment, which a read
/// sends.
pub fn here() -> BTreeMap<String, String> {
    SESSION
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect()
}

/// Run `f` with `vars` as the caller's environment on this thread.
pub fn as_caller<T>(vars: BTreeMap<String, String>, f: impl FnOnce() -> T) -> T {
    let before = CALLER.with(|c| c.borrow_mut().replace(vars));
    let ran = f();
    CALLER.with(|c| *c.borrow_mut() = before);
    ran
}

/// Run `f` with `folder` as the caller's working folder on this thread
/// (design D1): a relative path that the command names is read from it.
pub fn in_folder<T>(folder: &Path, f: impl FnOnce() -> T) -> T {
    let before = FOLDER.with(|c| c.borrow_mut().replace(folder.to_path_buf()));
    let ran = f();
    FOLDER.with(|c| *c.borrow_mut() = before);
    ran
}

/// The caller's working folder: the one a session's command names where
/// its call runs on this thread, else this process's own.
pub fn folder() -> std::io::Result<PathBuf> {
    match FOLDER.with(|c| c.borrow().clone()) {
        Some(folder) => Ok(folder),
        None => std::env::current_dir(),
    }
}

/// `path` as the caller names it: a relative path read from the caller's
/// working folder ([`folder`]).
pub fn path(path: &str) -> std::io::Result<PathBuf> {
    let path = Path::new(path);
    match path.is_absolute() {
        true => Ok(path.to_path_buf()),
        false => Ok(folder()?.join(path)),
    }
}
