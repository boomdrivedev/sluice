//! The caller's session, as a read names it to `sluice host` (the change
//! `sluice-host-answers-every-read`, design D1): the variables a driver
//! reads to find who asks, the agent's name and the pane it runs in.
//! `sluice host` answers a read on a thread of its own, so the variables
//! that the read names stand for the host process's own while the read's
//! call runs on that thread.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// The variables a read takes from its caller's environment.
pub const SESSION: [&str; 3] = ["SLUICE_AGENT", "HERDR_PANE_ID", "HERDR_SOCKET_PATH"];

thread_local! {
    /// The caller's variables while a read's call runs on this thread.
    static CALLER: RefCell<Option<BTreeMap<String, String>>> = const { RefCell::new(None) };
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
