//! The caller's session, as a read or a change names it to `sluice host`
//! (the change `sluice-host-answers-every-read`, design D1; the change
//! `sluice-host-makes-each-change`, design D1): the variables a driver
//! reads to find who asks, the agent's name and the pane it runs in, and,
//! for a change, the trace context it was started under. `sluice host`
//! answers on a thread of its own, so the variables that a request names
//! stand for the host process's own while the request's call runs on that
//! thread, and a path the request names relative to its folder is read
//! from that folder.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The variables a read takes from its caller's environment.
pub const SESSION: [&str; 3] = ["SLUICE_AGENT", "HERDR_PANE_ID", "HERDR_SOCKET_PATH"];

/// The variables a change takes beside [`SESSION`]: the trace context it
/// was started under, which its submission keeps and its span nests in
/// (153; design D1).
pub const TRACE: [&str; 2] = ["TRACEPARENT", "TRACESTATE"];

/// A request's caller, while its call runs on this thread.
struct Asking {
    vars: BTreeMap<String, String>,
    folder: Option<PathBuf>,
}

thread_local! {
    /// The caller's variables and folder while a request's call runs on
    /// this thread.
    static CALLER: RefCell<Option<Asking>> = const { RefCell::new(None) };
}

/// `name`, as the caller's environment holds it: the request's where a
/// request's call runs on this thread, else this process's own.
pub fn var(name: &str) -> Option<String> {
    let named = CALLER.with(|c| c.borrow().as_ref().map(|a| a.vars.get(name).cloned()));
    match named {
        Some(value) => value,
        None => std::env::var(name).ok(),
    }
}

/// `path` as the caller names it: from the request's folder where a
/// request's call runs on this thread and `path` is relative, else as it
/// is, read from this process's own folder.
pub fn path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let folder = CALLER.with(|c| c.borrow().as_ref().and_then(|a| a.folder.clone()));
    match folder {
        Some(folder) => folder
            .join(path)
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect(),
        None => path.to_path_buf(),
    }
}

/// The folder the caller runs in: the request's where a request's call
/// runs on this thread, else this process's own.
pub fn folder() -> std::io::Result<PathBuf> {
    match CALLER.with(|c| c.borrow().as_ref().and_then(|a| a.folder.clone())) {
        Some(folder) => Ok(folder),
        None => std::env::current_dir(),
    }
}

thread_local! {
    /// Within a wait's call in `sluice host`, the end of the spent
    /// allowance the wait has said already, which none of its steps says
    /// again (134; the change `sluice-host-makes-each-change`, design D2).
    static TOLD: std::cell::Cell<Option<Option<chrono::DateTime<chrono::Utc>>>> =
        const { std::cell::Cell::new(None) };
}

/// Run `f` as a step of a wait that has said a spent allowance until
/// `told` already: what it says in `f` it says once over every step. The
/// allowance the wait has said after `f`.
pub fn telling<T>(
    told: Option<chrono::DateTime<chrono::Utc>>,
    f: impl FnOnce() -> T,
) -> (T, Option<chrono::DateTime<chrono::Utc>>) {
    let before = TOLD.with(|t| t.replace(Some(told)));
    let ran = f();
    let after = TOLD.with(|t| t.replace(before)).flatten();
    (ran, after)
}

/// Whether a spent allowance until `until` is still to be said: always
/// outside a wait's step, and within one where the wait has not said it,
/// which it is from now on.
pub fn untold(until: chrono::DateTime<chrono::Utc>) -> bool {
    TOLD.with(|t| match t.get() {
        None => true,
        Some(Some(said)) if said == until => false,
        Some(_) => {
            t.set(Some(Some(until)));
            true
        }
    })
}

/// The session variables of this process's environment, which a read
/// sends.
pub fn here() -> BTreeMap<String, String> {
    vars_of(&SESSION)
}

/// The session variables and the trace context of this process's
/// environment, which a change sends (design D1).
pub fn traced() -> BTreeMap<String, String> {
    let mut vars = here();
    vars.extend(vars_of(&TRACE));
    vars
}

fn vars_of(names: &[&str]) -> BTreeMap<String, String> {
    names
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect()
}

/// Run `f` with `vars` as the caller's environment on this thread.
pub fn as_caller<T>(vars: BTreeMap<String, String>, f: impl FnOnce() -> T) -> T {
    in_folder(vars, None, f)
}

/// Run `f` with `vars` as the caller's environment, and `folder` as the
/// folder it runs in, on this thread (design D1).
pub fn in_folder<T>(
    vars: BTreeMap<String, String>,
    folder: Option<PathBuf>,
    f: impl FnOnce() -> T,
) -> T {
    let before = CALLER.with(|c| c.borrow_mut().replace(Asking { vars, folder }));
    let ran = f();
    CALLER.with(|c| *c.borrow_mut() = before);
    ran
}

#[cfg(test)]
mod tests {
    use super::*;

    /// design D1: a change's call answers `TRACEPARENT` and a relative
    /// path from its caller, and this process's own outside it.
    #[test]
    fn a_calls_trace_and_paths_are_its_callers() {
        let vars = BTreeMap::from([(
            "TRACEPARENT".to_string(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
        )]);
        let seen = in_folder(vars, Some(PathBuf::from("/w/feat")), || {
            (var("TRACEPARENT"), path("."), path("../a"), path("/x"))
        });
        assert_eq!(
            seen.0.as_deref(),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
        );
        assert_eq!(seen.1, PathBuf::from("/w/feat"));
        assert_eq!(seen.2, PathBuf::from("/w/feat/../a"));
        assert_eq!(seen.3, PathBuf::from("/x"));
        assert_eq!(path("."), PathBuf::from("."));
    }

    /// 134, design D2: a wait's steps say a spent allowance once over all
    /// of them, the mark carried from one step to the next; a new end is
    /// said again; outside a wait's step it is always said.
    #[test]
    fn a_wait_says_a_pause_once_over_its_steps() {
        let until = chrono::Utc::now();
        let ((first, again), told) = telling(None, || (untold(until), untold(until)));
        assert!(first && !again);
        assert_eq!(told, Some(until));
        let (next_step, told) = telling(told, || untold(until));
        assert!(!next_step, "a later step says it no more");
        let later = until + chrono::TimeDelta::minutes(5);
        let (new_end, told) = telling(told, || untold(later));
        assert!(new_end);
        assert_eq!(told, Some(later));
        assert!(untold(until) && untold(until), "outside a step, always");
    }
}
